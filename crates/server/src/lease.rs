//! Single-writer lease in R2 (docs/reliability.md §3.6).
//!
//! `lease.json` = `{generation, holder, renewed_at}`, written only with conditional PUTs.
//! - At start, a service that the lease already names resumes it (including the standby after its
//!   own Take over and restart).
//! - A service never takes a lease it doesn't hold, even an expired one: only Take over does,
//!   and Take over always restores the newest backup first. So a service can never come up
//!   writing on a stale disk (fail back is a Take over on the active).
//! - The very first start (no lease object at all) creates generation 1 for `active`.
//! - Renewal every 15 s with `If-Match`. A 412 with another holder fences this service at once.
//! - If R2 is unreachable the service keeps serving (the runbook suspends the old service first).

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use crate::r2::{Cond, R2Error, R2};

pub const KEY: &str = "lease.json";
pub const RENEW_EVERY: Duration = Duration::from_secs(15);
/// Shown on /health when renewals have failed this long (R2 unreachable).
pub const STALE_AFTER: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Lease {
    pub generation: i64,
    pub holder: String,
    pub renewed_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LeaseStatus {
    pub instance: String,
    pub fenced: bool,
    pub generation: i64,
    pub holder: Option<String>,
    pub reason: String,
    #[serde(skip)]
    pub etag: Option<String>,
    #[serde(skip)]
    pub last_ok: Option<Instant>,
    pub last_error: Option<String>,
}

#[derive(Clone)]
pub struct LeaseHandle {
    pub r2: R2,
    pub status: Arc<Mutex<LeaseStatus>>,
}

fn now_s() -> i64 {
    chrono::Utc::now().timestamp()
}

fn parse(body: &[u8]) -> Option<Lease> {
    serde_json::from_slice(body).ok()
}

impl LeaseHandle {
    pub fn is_fenced(&self) -> bool {
        self.status.lock().expect("lease poisoned").fenced
    }

    pub fn generation(&self) -> i64 {
        self.status.lock().expect("lease poisoned").generation
    }

    pub fn snapshot(&self) -> LeaseStatus {
        self.status.lock().expect("lease poisoned").clone()
    }

    fn set(&self, f: impl FnOnce(&mut LeaseStatus)) {
        f(&mut self.status.lock().expect("lease poisoned"));
    }

    fn fence(&self, reason: &str, holder: Option<String>) {
        tracing::error!(%reason, "FENCED: this service no longer accepts writes");
        self.set(|s| {
            s.fenced = true;
            s.reason = reason.to_string();
            s.holder = holder;
            s.etag = None;
        });
    }

    async fn write(&self, lease: &Lease, cond: Cond) -> Result<String, R2Error> {
        self.r2
            .put(KEY, serde_json::to_vec(lease).expect("lease json"), cond)
            .await
    }

    /// Decide at start whether this service may write. `was_holder` = this server's database
    /// records that it held the lease last time; used only when R2 can't be reached.
    pub async fn start(r2: R2, instance: &str) -> Self {
        Self::start_with(r2, instance, false).await
    }

    pub async fn start_with(r2: R2, instance: &str, was_holder: bool) -> Self {
        let h = LeaseHandle {
            r2,
            status: Arc::new(Mutex::new(LeaseStatus {
                instance: instance.to_string(),
                fenced: true,
                generation: 0,
                holder: None,
                reason: "starting".into(),
                etag: None,
                last_ok: None,
                last_error: None,
            })),
        };
        if instance != "active" {
            h.set(|s| s.reason = "this is the standby; use Take over".into());
            match h.r2.get(KEY).await {
                // After its own Take over and restart, the standby is the lease holder.
                Ok(Some((b, etag))) => {
                    if let Some(l) = parse(&b) {
                        if l.holder == instance {
                            let renewed = Lease {
                                renewed_at: now_s(),
                                ..l.clone()
                            };
                            match h.write(&renewed, Cond::IfMatch(etag)).await {
                                Ok(etag) => h.took(renewed, etag),
                                Err(e) => h.fence(&format!("could not resume the lease: {e}"), None),
                            }
                        } else {
                            h.set(|s| {
                                s.generation = l.generation;
                                s.holder = Some(l.holder);
                            });
                        }
                    }
                }
                Ok(None) => {}
                Err(e) => h.set(|s| s.last_error = Some(e.to_string())),
            }
            return h;
        }
        match h.r2.get(KEY).await {
            Ok(None) => {
                let l = Lease {
                    generation: 1,
                    holder: instance.into(),
                    renewed_at: now_s(),
                };
                match h.write(&l, Cond::IfNoneMatch).await {
                    Ok(etag) => h.took(l, etag),
                    Err(e) => h.fence(&format!("could not create the lease: {e}"), None),
                }
            }
            Ok(Some((b, etag))) => {
                let Some(cur) = parse(&b) else {
                    h.fence("lease.json is unreadable; fix it in R2", None);
                    return h;
                };
                let mine = cur.holder == instance;
                if mine {
                    let l = Lease {
                        generation: cur.generation,
                        holder: instance.into(),
                        renewed_at: now_s(),
                    };
                    match h.write(&l, Cond::IfMatch(etag)).await {
                        Ok(etag) => h.took(l, etag),
                        Err(R2Error::PreconditionFailed) => {
                            h.fence("another service took the lease first", None)
                        }
                        Err(e) => h.fence(&format!("could not take the lease: {e}"), None),
                    }
                } else {
                    h.fence(
                        &format!("the lease is held by '{}'; use Take over to make this server active", cur.holder),
                        Some(cur.holder),
                    );
                }
            }
            // R2 unreachable at start: serve only if this server held the lease last time
            // (an ordinary restart during an R2 outage), with a warning on /health.
            Err(e) if was_holder => h.set(|s| {
                s.fenced = false;
                s.reason = format!("lease unconfirmed, R2 unreachable: {e}");
                s.last_error = Some(e.to_string());
            }),
            Err(e) => h.fence(&format!("R2 unreachable and this server wasn't the last holder: {e}"), None),
        }
        h
    }

    fn took(&self, l: Lease, etag: String) {
        tracing::info!(generation = l.generation, "lease held");
        self.set(|s| {
            s.fenced = false;
            s.generation = l.generation;
            s.holder = Some(l.holder);
            s.reason = "lease held".into();
            s.etag = Some(etag);
            s.last_ok = Some(Instant::now());
            s.last_error = None;
        });
    }

    /// One renewal attempt (the loop calls this every 15 s while not fenced).
    pub async fn renew(&self) {
        let st = self.snapshot();
        if st.fenced {
            return;
        }
        let l = Lease {
            generation: st.generation.max(1),
            holder: st.instance.clone(),
            renewed_at: now_s(),
        };
        let cond = match st.etag {
            Some(e) => Cond::IfMatch(e),
            // Started while R2 was down: try to take it properly now.
            None => {
                return self.retake(&st.instance).await;
            }
        };
        match self.write(&l, cond).await {
            Ok(etag) => self.took(l, etag),
            Err(R2Error::PreconditionFailed) => {
                // Someone else wrote the lease. Who?
                match self.r2.get(KEY).await {
                    Ok(Some((b, etag))) => match parse(&b) {
                        Some(cur) if cur.holder == st.instance && cur.generation == l.generation => {
                            self.set(|s| s.etag = Some(etag));
                        }
                        Some(cur) => self.fence(
                            &format!("'{}' took over (generation {})", cur.holder, cur.generation),
                            Some(cur.holder),
                        ),
                        None => self.fence("lease.json became unreadable", None),
                    },
                    _ => self.fence("the lease changed and could not be read", None),
                }
            }
            Err(e) => self.set(|s| s.last_error = Some(e.to_string())),
        }
    }

    async fn retake(&self, instance: &str) {
        match self.r2.get(KEY).await {
            Ok(None) => {
                let l = Lease {
                    generation: 1,
                    holder: instance.into(),
                    renewed_at: now_s(),
                };
                if let Ok(etag) = self.write(&l, Cond::IfNoneMatch).await {
                    self.took(l, etag);
                }
            }
            Ok(Some((b, etag))) => match parse(&b) {
                Some(cur) if cur.holder == instance => {
                    let l = Lease {
                        generation: cur.generation,
                        holder: instance.into(),
                        renewed_at: now_s(),
                    };
                    if let Ok(etag) = self.write(&l, Cond::IfMatch(etag)).await {
                        self.took(l, etag);
                    }
                }
                Some(cur) => self.fence(&format!("the lease is held by '{}'", cur.holder), Some(cur.holder)),
                None => self.fence("lease.json is unreadable", None),
            },
            Err(e) => self.set(|s| s.last_error = Some(e.to_string())),
        }
    }

    /// Take over (standby runbook step 3): write the lease with the next generation, conditional
    /// on the version just read, so two simultaneous takeovers can't both win.
    pub async fn take_over(&self) -> Result<i64, String> {
        let instance = self.snapshot().instance;
        let (cur, etag) = match self.r2.get(KEY).await.map_err(|e| e.to_string())? {
            Some((b, etag)) => (parse(&b).ok_or("lease.json is unreadable")?, Some(etag)),
            None => (
                Lease {
                    generation: 0,
                    holder: String::new(),
                    renewed_at: 0,
                },
                None,
            ),
        };
        let l = Lease {
            generation: cur.generation + 1,
            holder: instance,
            renewed_at: now_s(),
        };
        let cond = etag.map(Cond::IfMatch).unwrap_or(Cond::IfNoneMatch);
        match self.write(&l, cond).await {
            Ok(etag) => {
                let g = l.generation;
                self.took(l, etag);
                // Still serving the old (stale) database until the restart: refuse writes here.
                self.set(|s| {
                    s.fenced = true;
                    s.reason = "took over; restarting into the restored backup".into();
                });
                Ok(g)
            }
            Err(R2Error::PreconditionFailed) => {
                Err("someone else changed the lease at the same moment; try again".into())
            }
            Err(e) => Err(e.to_string()),
        }
    }

    pub async fn run(self) {
        loop {
            tokio::time::sleep(RENEW_EVERY).await;
            self.renew().await;
        }
    }
}
