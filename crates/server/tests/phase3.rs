//! Phase 3: swaps, PIN sign-in, show-mode lock, battery rules, CSV and show import/export,
//! backups, post-show report.

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use rfops_server::{app, db, rules, seed, AppState};
use serde_json::{json, Value};
use tower::ServiceExt;

struct T {
    app: Router,
    pool: sqlx::SqlitePool,
    actor: String,
    token: Option<String>,
}

impl T {
    async fn demo() -> Self {
        let pool = db::connect("sqlite::memory:").await.unwrap();
        seed::seed_demo(&pool).await.unwrap();
        Self::with_state(AppState::new(pool.clone()).await.unwrap(), pool)
    }

    fn with_state(state: AppState, pool: sqlx::SqlitePool) -> Self {
        T {
            app: app(state),
            pool,
            actor: "Admin".into(),
            token: None,
        }
    }

    async fn raw(
        &self,
        method: &str,
        uri: &str,
        body: Option<String>,
        ctype: &str,
    ) -> (StatusCode, Value, String) {
        let mut b = Request::builder()
            .method(method)
            .uri(uri)
            .header("x-rfops-actor", &self.actor);
        if let Some(t) = &self.token {
            b = b.header("x-rfops-token", t);
        }
        let req = match body {
            Some(v) => b.header("content-type", ctype).body(Body::from(v)),
            None => b.body(Body::empty()),
        }
        .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8_lossy(&bytes).to_string();
        (
            status,
            serde_json::from_str(&text).unwrap_or(Value::Null),
            text,
        )
    }

    async fn req(&self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let (s, v, _) = self
            .raw(method, uri, body.map(|b| b.to_string()), "application/json")
            .await;
        (s, v)
    }

    async fn ok(&self, method: &str, uri: &str, body: Option<Value>) -> Value {
        let (s, v) = self.req(method, uri, body).await;
        assert!(s.is_success(), "{method} {uri}: {s} {v}");
        v
    }

    async fn snapshot(&self) -> Value {
        let show = self.show_id().await;
        self.ok("GET", &format!("/api/snapshot?show_id={show}"), None)
            .await
    }

    async fn show_id(&self) -> i64 {
        sqlx::query_scalar("SELECT id FROM shows ORDER BY id LIMIT 1")
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    async fn id_of(&self, table: &str, col: &str, val: &str) -> i64 {
        sqlx::query_scalar(&format!("SELECT id FROM {table} WHERE {col} = ?"))
            .bind(val)
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    async fn song(&self, title: &str) -> i64 {
        self.id_of("songs", "title", title).await
    }
}

#[tokio::test]
async fn swap_rest_of_show_moves_performer_and_marks_faulty() {
    let t = T::demo().await;
    let show = t.show_id().await;
    let maya = t.id_of("performers", "name", "Maya").await;
    let vox1 = t.id_of("kits", "name", "Vox 1").await;
    let spare = t.id_of("kits", "name", "Spare HH A").await;
    let hh2 = t.id_of("equipment", "label", "HH2").await;
    let song2 = t.song("Song 2").await;
    t.ok(
        "PATCH",
        &format!("/api/shows/{show}"),
        Some(json!({ "mode": "show" })),
    )
    .await;
    t.ok(
        "POST",
        &format!("/api/shows/{show}/current"),
        Some(json!({ "song_id": song2 })),
    )
    .await;

    let r = t
        .ok(
            "POST",
            &format!("/api/shows/{show}/swap"),
            Some(json!({ "performer_id": maya, "from_kit_id": vox1, "to_kit_id": spare, "scope": "rest_of_show", "mark_faulty": true, "reason": "dropout" })),
        )
        .await;
    // Maya sings Song 2, 3, Ballad, Song 5 (off for Message and Closing).
    assert_eq!(r["songs_changed"].as_array().unwrap().len(), 4, "{r}");
    assert_eq!(r["faulty_item_id"], hh2);

    let snap = t.snapshot().await;
    let s = &snap["show"];
    assert_eq!(
        s["effective"][maya.to_string()]
            .as_array()
            .unwrap()
            .iter()
            .filter(|k| **k == spare)
            .count(),
        1
    );
    assert!(
        s["conflicts"]["by_song"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["conflicts"]
                .as_array()
                .unwrap()
                .iter()
                .all(|c| c["type"] != "item_shared")),
        "{}",
        s["conflicts"]
    );
    assert_eq!(
        s["recent_swaps"][0]["payload"]["to_kit"]["console_channel"],
        9
    );
    assert_eq!(
        s["recent_swaps"][0]["payload"]["from_kit"]["console_channel"],
        2
    );
    let status: String = sqlx::query_scalar("SELECT status FROM equipment WHERE id = ?")
        .bind(hh2)
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(status, "faulty");
    // Earlier songs untouched.
    let s1 = t.song("Song 1 — Opener").await;
    let eff = t
        .ok(
            "GET",
            &format!("/api/shows/{show}/effective?song_id={s1}"),
            None,
        )
        .await;
    assert_eq!(
        eff[maya.to_string()]
            .as_array()
            .unwrap()
            .iter()
            .filter(|k| **k == vox1)
            .count(),
        1
    );
}

#[tokio::test]
async fn swap_rejects_spare_in_use_and_song_scope_is_one_song() {
    let t = T::demo().await;
    let show = t.show_id().await;
    let maya = t.id_of("performers", "name", "Maya").await;
    let chris = t.id_of("performers", "name", "Chris").await;
    let vox1 = t.id_of("kits", "name", "Vox 1").await;
    let vox2 = t.id_of("kits", "name", "Vox 2").await;
    let spare = t.id_of("kits", "name", "Spare HH B").await;
    let s1 = t.song("Song 1 — Opener").await;
    t.ok(
        "POST",
        &format!("/api/shows/{show}/current"),
        Some(json!({ "song_id": s1 })),
    )
    .await;

    let (st, v) = t
        .req(
            "POST",
            &format!("/api/shows/{show}/swap"),
            Some(json!({ "performer_id": maya, "from_kit_id": vox1, "to_kit_id": vox2 })),
        )
        .await;
    assert_eq!(st, StatusCode::CONFLICT, "Vox 2 is Chris's: {v}");
    assert!(v["error"].as_str().unwrap().contains("Chris"));

    let r = t
        .ok("POST", &format!("/api/shows/{show}/swap"), Some(json!({ "performer_id": maya, "from_kit_id": vox1, "to_kit_id": spare, "scope": "song" })))
        .await;
    assert_eq!(r["songs_changed"], json!([s1]));
    assert!(r["faulty_item_id"].is_null());
    let s2 = t.song("Song 2").await;
    let eff = t
        .ok(
            "GET",
            &format!("/api/shows/{show}/effective?song_id={s2}"),
            None,
        )
        .await;
    assert!(eff[maya.to_string()]
        .as_array()
        .unwrap()
        .contains(&json!(vox1)));

    // Wrong performer for the kit.
    let (st, _) = t
        .req("POST", &format!("/api/shows/{show}/swap"), Some(json!({ "performer_id": chris, "from_kit_id": vox1, "to_kit_id": t.id_of("kits", "name", "Spare HH A").await })))
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn pin_sign_in_is_required_once_set() {
    let mut t = T::demo().await;
    let admin = t.id_of("users", "name", "Admin").await;
    let left = t.id_of("users", "name", "A2 Stage Left").await;

    // A2 cannot set someone else's PIN.
    t.actor = "A2 Stage Left".into();
    let (st, _) = t
        .req(
            "PUT",
            &format!("/api/users/{admin}/pin"),
            Some(json!({ "pin": "1234" })),
        )
        .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    // But can set their own.
    let (st, v) = t
        .req(
            "PUT",
            &format!("/api/users/{left}/pin"),
            Some(json!({ "pin": "12" })),
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    let v = t
        .ok(
            "PUT",
            &format!("/api/users/{left}/pin"),
            Some(json!({ "pin": "4821" })),
        )
        .await;
    assert_eq!(v["has_pin"], true);

    let alert = json!({ "severity": "info", "title": "x" });
    let (st, _) = t.req("POST", "/api/alerts", Some(alert.clone())).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED, "PIN set, no token");

    let (st, _) = t
        .req(
            "POST",
            "/api/login",
            Some(json!({ "name": "A2 Stage Left", "pin": "0000" })),
        )
        .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let login = t
        .ok(
            "POST",
            "/api/login",
            Some(json!({ "name": "A2 Stage Left", "pin": "4821" })),
        )
        .await;
    t.token = Some(login["token"].as_str().unwrap().to_string());
    t.ok("POST", "/api/alerts", Some(alert.clone())).await;

    // A token for one operator does not work for another with a PIN.
    t.actor = "Admin".into();
    t.ok(
        "PUT",
        &format!("/api/users/{admin}/pin"),
        Some(json!({ "pin": "9999" })),
    )
    .await;
    let (st, _) = t.req("POST", "/api/alerts", Some(alert.clone())).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);

    // Logout invalidates.
    t.actor = "A2 Stage Left".into();
    t.ok("POST", "/api/logout", None).await;
    let (st, _) = t.req("POST", "/api/alerts", Some(alert)).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);

    let snap = t.snapshot().await;
    let users = snap["users"].as_array().unwrap();
    assert!(users
        .iter()
        .any(|u| u["name"] == "A2 Stage Left" && u["has_pin"] == true));
    assert!(
        users.iter().all(|u| u.get("pin_hash").is_none()),
        "hash never leaves the server"
    );
}

#[tokio::test]
async fn show_mode_locks_structure_but_not_operations() {
    let t = T::demo().await;
    let show = t.show_id().await;
    let sam = t.id_of("performers", "name", "Sam").await;
    let hh = t.id_of("equipment", "label", "HH5").await;
    let rx = t.id_of("equipment", "label", "RX3-4").await;
    t.ok(
        "PATCH",
        &format!("/api/shows/{show}"),
        Some(json!({ "mode": "show" })),
    )
    .await;

    let (st, v) = t
        .req(
            "POST",
            &format!("/api/shows/{show}/kits"),
            Some(json!({ "kind": "mic", "name": "x", "base_id": rx, "carrier_id": hh })),
        )
        .await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["error"].as_str().unwrap().contains("Show mode"));
    let (st, _) = t
        .req(
            "PUT",
            &format!("/api/shows/{show}/assignments/cell"),
            Some(json!({ "performer_id": sam, "kind": "mic", "song_id": null, "kit_ids": [] })),
        )
        .await;
    assert_eq!(st, StatusCode::CONFLICT);
    let (st, _) = t.req("DELETE", &format!("/api/shows/{show}"), None).await;
    assert_eq!(st, StatusCode::CONFLICT);

    // Live operations still work.
    t.ok(
        "POST",
        "/api/battery-checks",
        Some(json!({ "equipment_id": hh, "result": "fresh" })),
    )
    .await;
    t.ok(
        "POST",
        "/api/alerts",
        Some(json!({ "severity": "warning", "title": "test" })),
    )
    .await;
    t.ok(
        "POST",
        &format!("/api/shows/{show}/advance"),
        Some(json!({ "direction": "next" })),
    )
    .await;
    let s3 = t.song("Song 3").await;
    t.ok(
        "PATCH",
        &format!("/api/songs/{s3}"),
        Some(json!({ "skipped": true })),
    )
    .await;

    t.ok(
        "PATCH",
        &format!("/api/shows/{show}"),
        Some(json!({ "mode": "rehearsal" })),
    )
    .await;
    t.ok(
        "POST",
        &format!("/api/shows/{show}/kits"),
        Some(json!({ "kind": "mic", "name": "x", "base_id": rx, "carrier_id": hh })),
    )
    .await;
}

#[tokio::test]
async fn battery_rule_opens_and_clears_alerts_in_show_mode() {
    let t = T::demo().await;
    let show = t.show_id().await;
    assert_eq!(
        rules::evaluate(&t.pool).await.unwrap(),
        0,
        "no rules outside show mode"
    );
    t.ok(
        "PATCH",
        &format!("/api/shows/{show}"),
        Some(json!({ "mode": "show" })),
    )
    .await;
    // Entering show mode evaluates immediately: every pack in use is unchecked.
    let active: Vec<(String, String)> = sqlx::query_as(
        "SELECT title, severity FROM alerts WHERE source = 'rule' AND state <> 'resolved'",
    )
    .fetch_all(&t.pool)
    .await
    .unwrap();
    assert!(active.len() >= 15, "got {}", active.len());
    assert!(active
        .iter()
        .any(|(title, _)| title == "Battery not checked: HH1"));
    // Spares are not in use, so no alert for them.
    assert!(!active.iter().any(|(title, _)| title.ends_with("HH7")));
    assert_eq!(rules::evaluate(&t.pool).await.unwrap(), 0, "deduplicated");

    let hh1 = t.id_of("equipment", "label", "HH1").await;
    t.ok(
        "POST",
        "/api/battery-checks",
        Some(json!({ "equipment_id": hh1, "result": "fresh" })),
    )
    .await;
    rules::evaluate(&t.pool).await.unwrap();
    let state: String =
        sqlx::query_scalar("SELECT state FROM alerts WHERE title = 'Battery not checked: HH1'")
            .fetch_one(&t.pool)
            .await
            .unwrap();
    assert_eq!(state, "resolved");

    // A low pack on stage now is critical.
    let s1 = t.song("Song 1 — Opener").await;
    t.ok(
        "POST",
        &format!("/api/shows/{show}/current"),
        Some(json!({ "song_id": s1 })),
    )
    .await;
    t.ok(
        "POST",
        "/api/battery-checks",
        Some(json!({ "equipment_id": hh1, "result": "low" })),
    )
    .await;
    rules::evaluate(&t.pool).await.unwrap();
    let sev: String = sqlx::query_scalar(
        "SELECT severity FROM alerts WHERE title = 'Battery LOW: HH1' AND state <> 'resolved'",
    )
    .fetch_one(&t.pool)
    .await
    .unwrap();
    assert_eq!(sev, "critical");

    t.ok(
        "PATCH",
        &format!("/api/shows/{show}"),
        Some(json!({ "mode": "rehearsal" })),
    )
    .await;
    rules::evaluate(&t.pool).await.unwrap();
    let left: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM alerts WHERE source = 'rule' AND state <> 'resolved'",
    )
    .fetch_one(&t.pool)
    .await
    .unwrap();
    assert_eq!(left, 0);
}

#[tokio::test]
async fn equipment_and_performer_csv_round_trip() {
    let t = T::demo().await;
    let (st, _, csv) = t.raw("GET", "/api/export/equipment.csv", None, "").await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        csv.starts_with("kind,make,model,label,serial,parent,channel_no,ip_address,status,notes")
    );
    assert!(csv.contains("receiver_channel,Shure,ULXD4Q,RX1-1,,RX1,1,,available,"));

    // Update one, add a receiver with a channel that references it.
    let upload = "kind,make,model,label,serial,parent,channel_no,ip_address,status,notes\n\
                  handheld_tx,Shure,ULXD2,HH1,SN-001,,,,available,new serial\n\
                  receiver,Shure,ULXD4Q,RX4,,,,192.168.1.24,available,\n\
                  receiver_channel,Shure,ULXD4Q,RX4-1,,RX4,1,,available,\n";
    let (st, v, _) = t
        .raw(
            "POST",
            "/api/import/equipment",
            Some(upload.into()),
            "text/csv",
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["created"], 2);
    assert_eq!(v["updated"], 1);
    let serial: Option<String> =
        sqlx::query_scalar("SELECT serial FROM equipment WHERE label = 'HH1'")
            .fetch_one(&t.pool)
            .await
            .unwrap();
    assert_eq!(serial.as_deref(), Some("SN-001"));
    let parent: Option<String> = sqlx::query_scalar("SELECT p.label FROM equipment e JOIN equipment p ON p.id = e.parent_id WHERE e.label = 'RX4-1'")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(parent.as_deref(), Some("RX4"));

    // Errors reject the whole file.
    let bad = "kind,make,model,label,serial,parent,channel_no,ip_address,status,notes\n\
               handheld_tx,Shure,ULXD2,HH99,,,,,available,\n\
               toaster,,,T1,,,,,,\n\
               capsule,,,C1,,NOPE,,,,\n";
    let (st, v, _) = t
        .raw(
            "POST",
            "/api/import/equipment",
            Some(bad.into()),
            "text/csv",
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert_eq!(v["errors"].as_array().unwrap().len(), 2, "{v}");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM equipment WHERE label = 'HH99'")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(n, 0);

    let (_, _, pcsv) = t.raw("GET", "/api/export/performers.csv", None, "").await;
    assert!(pcsv.contains("Maya,Vocal 1,Stage left,A2 Stage Left,"));
    let up = "name,role,zone,a2,notes\nMaya,Vocal 1 (alto),Stage left,A2 Stage Right,\nNew Person,Choir,,,\n";
    let (st, v, _) = t
        .raw(
            "POST",
            "/api/import/performers",
            Some(up.into()),
            "text/csv",
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(
        (v["created"].as_i64(), v["updated"].as_i64()),
        (Some(1), Some(1))
    );

    let show = t.show_id().await;
    let (st, _, plot) = t
        .raw(
            "GET",
            &format!("/api/shows/{show}/export/plot.csv"),
            None,
            "",
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    assert!(plot.starts_with("position,song,performer,type,kit,console,receiver,pack,element"));
    assert!(
        plot.contains("Song 4 — Ballad,Jordan,mic,Lead Vox KSM9,Ch 1,RX1-1,HH1,KSM9-1"),
        "{plot}"
    );
}

#[tokio::test]
async fn show_export_import_round_trip() {
    let t = T::demo().await;
    let show = t.show_id().await;
    let (st, file, _) = t
        .raw("GET", &format!("/api/shows/{show}/export"), None, "")
        .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(file["format"], "rfops-show/1");

    let r = t
        .ok(
            "POST",
            "/api/shows/import",
            Some(json!({ "file": file, "name": "Imported" })),
        )
        .await;
    let new_id = r["show"]["id"].as_i64().unwrap();
    assert_eq!(r["performers_created"], json!([]));
    let a = t
        .ok("GET", &format!("/api/shows/{show}/handoffs"), None)
        .await;
    let b = t
        .ok("GET", &format!("/api/shows/{new_id}/handoffs"), None)
        .await;
    let count = |v: &Value| {
        v.as_array()
            .unwrap()
            .iter()
            .map(|t| t["handoffs"].as_array().unwrap().len())
            .sum::<usize>()
    };
    assert_eq!(count(&a), count(&b));
    let c = t
        .ok("GET", &format!("/api/shows/{new_id}/conflicts"), None)
        .await;
    assert!(c["by_song"].as_array().unwrap().is_empty());

    // Unknown equipment is reported, nothing created.
    let mut bad = file.clone();
    bad["kits"][0]["carrier"] = json!("NOT-A-PACK");
    let (st, v) = t
        .req("POST", "/api/shows/import", Some(json!({ "file": bad })))
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert!(v["error"].as_str().unwrap().contains("NOT-A-PACK"));
    let shows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM shows")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(shows, 2);
}

#[tokio::test]
async fn backups_and_report() {
    let dir = std::env::temp_dir().join(format!("rfops-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let pool = db::connect(&format!("sqlite://{}", dir.join("live.db").display()))
        .await
        .unwrap();
    seed::seed_demo(&pool).await.unwrap();
    let backups = dir.join("backups");
    let state = AppState::new(pool.clone()).await.unwrap().with_backups(
        backups.clone(),
        std::time::Duration::from_secs(3600),
        std::time::Duration::from_secs(3600),
    );
    let t = T::with_state(state, pool);
    let show = t.show_id().await;

    let none = t.ok("GET", "/api/backups", None).await;
    assert_eq!(none["files"].as_array().unwrap().len(), 0);
    t.ok(
        "PATCH",
        &format!("/api/shows/{show}"),
        Some(json!({ "mode": "show" })),
    )
    .await;
    let f = t.ok("POST", "/api/backups", None).await;
    assert!(f["bytes"].as_u64().unwrap() > 10_000);
    let list = t.ok("GET", "/api/backups", None).await;
    let names: Vec<&str> = list["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(
        names.iter().any(|n| n.ends_with("-show-start-pin.db")),
        "show-start backups are pinned"
    );
    assert!(list["files"]
        .as_array()
        .unwrap()
        .iter()
        .all(|f| f["verified"] == true));
    // The backup is a working database.
    let copy = db::connect(&format!("sqlite://{}", backups.join(names[0]).display()))
        .await
        .unwrap();
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM performers")
        .fetch_one(&copy)
        .await
        .unwrap();
    assert_eq!(n, 11);

    // Report covers swaps, alerts and song changes.
    let maya = t.id_of("performers", "name", "Maya").await;
    let vox1 = t.id_of("kits", "name", "Vox 1").await;
    let spare = t.id_of("kits", "name", "Spare HH A").await;
    t.ok(
        "POST",
        &format!("/api/shows/{show}/advance"),
        Some(json!({ "direction": "next" })),
    )
    .await;
    t.ok(
        "POST",
        &format!("/api/shows/{show}/advance"),
        Some(json!({ "direction": "next" })),
    )
    .await;
    let a = t.ok("POST", "/api/alerts", Some(json!({ "severity": "critical", "title": "Vox 1 dropout", "show_id": show, "performer_id": maya }))).await;
    t.ok(
        "POST",
        &format!("/api/shows/{show}/swap"),
        Some(json!({ "performer_id": maya, "from_kit_id": vox1, "to_kit_id": spare, "alert_id": a["id"], "mark_faulty": true })),
    )
    .await;
    let rep = t
        .ok("GET", &format!("/api/shows/{show}/report"), None)
        .await;
    assert_eq!(rep["swaps"][0]["performer"], "Maya");
    assert_eq!(rep["swaps"][0]["from_kit"], "Vox 1");
    assert_eq!(rep["swaps"][0]["to_kit"], "Spare HH A");
    assert!(rep["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|x| x["title"] == "Vox 1 dropout" && x["state"] == "acknowledged"));
    assert_eq!(rep["song_changes"].as_array().unwrap().len(), 2);
    assert!(rep["faulty_items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|x| x["label"] == "HH2"));
    let _ = std::fs::remove_dir_all(&dir);
}
