use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use rfops_server::{app, db, AppState};
use serde_json::{json, Value};
use tower::ServiceExt;

struct T {
    app: Router,
}

impl T {
    async fn new() -> Self {
        let pool = db::connect("sqlite::memory:").await.unwrap();
        T {
            app: app(AppState { pool }),
        }
    }

    async fn req(
        &self,
        method: &str,
        uri: &str,
        body: Option<Value>,
        actor: bool,
    ) -> (StatusCode, Value) {
        let mut b = Request::builder().method(method).uri(uri);
        if actor {
            b = b
                .header("x-rfops-actor", "tester")
                .header("x-rfops-client", "test-client");
        }
        let req = match body {
            Some(v) => b
                .header("content-type", "application/json")
                .body(Body::from(v.to_string())),
            None => b.body(Body::empty()),
        }
        .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let v = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        (status, v)
    }

    async fn get(&self, uri: &str) -> Value {
        let (s, v) = self.req("GET", uri, None, false).await;
        assert_eq!(s, StatusCode::OK, "GET {uri}: {v}");
        v
    }

    async fn post(&self, uri: &str, body: Value) -> Value {
        let (s, v) = self.req("POST", uri, Some(body), true).await;
        assert!(s.is_success(), "POST {uri}: {s} {v}");
        v
    }

    async fn id(&self, uri: &str, body: Value) -> i64 {
        self.post(uri, body).await["id"].as_i64().unwrap()
    }

    async fn equipment(&self, kind: &str, label: &str) -> i64 {
        self.id("/api/equipment", json!({ "kind": kind, "label": label }))
            .await
    }
}

#[tokio::test]
async fn health() {
    let t = T::new().await;
    assert_eq!(t.get("/api/health").await["ok"], true);
}

#[tokio::test]
async fn mutations_require_actor() {
    let t = T::new().await;
    let (s, v) = t
        .req(
            "POST",
            "/api/performers",
            Some(json!({ "name": "A" })),
            false,
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(v["error"].as_str().unwrap().contains("x-rfops-actor"));
}

#[tokio::test]
async fn equipment_validation_and_crud() {
    let t = T::new().await;
    let (s, _) = t
        .req(
            "POST",
            "/api/equipment",
            Some(json!({ "kind": "toaster", "label": "X" })),
            true,
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let rx = t.id("/api/equipment", json!({ "kind": "receiver", "make": "Shure", "model": "ULXD4Q", "label": "RX1", "ip_address": "10.0.0.11" })).await;
    let ch = t.id("/api/equipment", json!({ "kind": "receiver_channel", "label": "RX1-1", "parent_id": rx, "channel_no": 1 })).await;

    // duplicate label
    let (s, _) = t
        .req(
            "POST",
            "/api/equipment",
            Some(json!({ "kind": "capsule", "label": "RX1" })),
            true,
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);

    let (s, v) = t
        .req(
            "PATCH",
            &format!("/api/equipment/{ch}"),
            Some(json!({ "notes": "front rack", "parent_id": null })),
            true,
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["notes"], "front rack");
    assert!(v["parent_id"].is_null());

    let list = t.get("/api/equipment?kind=receiver").await;
    assert_eq!(list.as_array().unwrap().len(), 1);

    let (s, _) = t
        .req("DELETE", &format!("/api/equipment/{ch}"), None, true)
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _) = t
        .req("GET", &format!("/api/equipment/{ch}"), None, false)
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

/// End-to-end planning: two performers share a handheld across songs, one capsule swap.
#[tokio::test]
async fn show_planning_flow() {
    let t = T::new().await;
    let ch1 = t.equipment("receiver_channel", "RX1-1").await;
    let ch2 = t.equipment("receiver_channel", "RX1-2").await;
    let hh1 = t.equipment("handheld_tx", "HH1").await;
    let bp1 = t.equipment("bodypack_tx", "BP1").await;
    let sm58 = t.equipment("capsule", "SM58-1").await;
    let b87 = t.equipment("capsule", "B87A-1").await;
    let hs = t.equipment("headset", "HS-1").await;
    let iem_ch = t.equipment("iem_tx_channel", "IEM-1").await;
    let pack = t.equipment("iem_pack", "EK-1").await;

    let alice = t
        .id(
            "/api/performers",
            json!({ "name": "Alice", "role": "Lead vocal" }),
        )
        .await;
    let bob = t
        .id("/api/performers", json!({ "name": "Bob", "role": "BGV" }))
        .await;

    let show = t
        .id(
            "/api/shows",
            json!({ "name": "Sunday AM", "call_time": "2026-11-22T13:00:00Z" }),
        )
        .await;
    let s1 = t
        .id(
            &format!("/api/shows/{show}/songs"),
            json!({ "title": "Opener" }),
        )
        .await;
    let s2 = t
        .id(
            &format!("/api/shows/{show}/songs"),
            json!({ "title": "Ballad" }),
        )
        .await;
    let s3 = t
        .id(
            &format!("/api/shows/{show}/songs"),
            json!({ "title": "Closer" }),
        )
        .await;

    // kit slot validation: capsule as carrier is rejected
    let (s, v) = t
        .req(
            "POST",
            &format!("/api/shows/{show}/kits"),
            Some(json!({ "kind": "mic", "name": "bad", "base_id": ch1, "carrier_id": sm58 })),
            true,
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");

    let k_hh_58 = t
        .id(&format!("/api/shows/{show}/kits"), json!({ "kind": "mic", "name": "HH1 SM58", "base_id": ch1, "carrier_id": hh1, "element_id": sm58, "console_channel": 1 }))
        .await;
    let k_hh_87 = t
        .id(&format!("/api/shows/{show}/kits"), json!({ "kind": "mic", "name": "HH1 B87A", "base_id": ch1, "carrier_id": hh1, "element_id": b87, "console_channel": 1 }))
        .await;
    let k_bp = t
        .id(&format!("/api/shows/{show}/kits"), json!({ "kind": "mic", "name": "BP1 HS", "base_id": ch2, "carrier_id": bp1, "element_id": hs, "console_channel": 2 }))
        .await;
    let k_iem = t
        .id(&format!("/api/shows/{show}/kits"), json!({ "kind": "iem", "name": "IEM 1", "base_id": iem_ch, "carrier_id": pack, "iem_mix": "Aux 9" }))
        .await;

    // Defaults: Alice handheld w/ SM58 + IEM; Bob headset.
    t.post(
        &format!("/api/shows/{show}/assignments"),
        json!({ "performer_id": alice, "kind": "mic", "kit_id": k_hh_58 }),
    )
    .await;
    t.post(
        &format!("/api/shows/{show}/assignments"),
        json!({ "performer_id": alice, "kind": "iem", "kit_id": k_iem }),
    )
    .await;
    t.post(
        &format!("/api/shows/{show}/assignments"),
        json!({ "performer_id": bob, "kind": "mic", "kit_id": k_bp }),
    )
    .await;

    // Song 2: Alice switches to B87A capsule.
    t.post(
        &format!("/api/shows/{show}/assignments"),
        json!({ "performer_id": alice, "kind": "mic", "kit_id": k_hh_87, "song_id": s2 }),
    )
    .await;

    // Song 3: Bob takes the handheld (B87A); forgetting to release Alice -> conflict reported.
    let r = t
        .post(
            &format!("/api/shows/{show}/assignments"),
            json!({ "performer_id": bob, "kind": "mic", "kit_id": k_hh_87, "song_id": s3 }),
        )
        .await;
    assert!(
        !r["conflicts"].as_array().unwrap().is_empty(),
        "expected conflict: {r}"
    );
    t.post(
        &format!("/api/shows/{show}/assignments"),
        json!({ "performer_id": alice, "kind": "mic", "kit_id": null, "song_id": s3 }),
    )
    .await;

    let conflicts = t.get(&format!("/api/shows/{show}/conflicts")).await;
    assert!(
        conflicts["by_song"].as_array().unwrap().is_empty(),
        "{conflicts}"
    );

    let eff = t
        .get(&format!("/api/shows/{show}/effective?song_id={s3}"))
        .await;
    assert_eq!(eff[bob.to_string()], json!([k_hh_87]));
    assert_eq!(eff[alice.to_string()], json!([k_iem]));

    let plan = t.get(&format!("/api/shows/{show}/handoffs")).await;
    let plan = plan.as_array().unwrap();
    // pre-show, 1->2 (capsule swap), 2->3 (handheld to Bob, bodypack returned), end
    assert_eq!(plan.len(), 4, "{plan:?}");
    let into2 = plan.iter().find(|t| t["to_song"] == s2).unwrap();
    assert!(into2["handoffs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|h| h["type"] == "component_change"));
    let into3 = plan.iter().find(|t| t["to_song"] == s3).unwrap();
    assert!(into3["handoffs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|h| h["type"] == "transfer"
            && h["item_id"] == hh1
            && h["from_performer"] == alice
            && h["to_performer"] == bob));

    // Current song + upcoming
    t.post(
        &format!("/api/shows/{show}/current"),
        json!({ "song_id": s1 }),
    )
    .await;
    let up = t.get(&format!("/api/shows/{show}/upcoming?count=1")).await;
    assert_eq!(up[0]["from_song"], s1);
    assert_eq!(up[0]["to_song"], s2);

    // Skipping song 2 removes its transitions from the plan.
    let (s, _) = t
        .req(
            "PATCH",
            &format!("/api/songs/{s2}"),
            Some(json!({ "skipped": true })),
            true,
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let plan = t.get(&format!("/api/shows/{show}/handoffs")).await;
    assert!(plan.as_array().unwrap().iter().all(|t| t["to_song"] != s2));

    // Duplicate as a template keeps the structure.
    let dup = t
        .id(
            &format!("/api/shows/{show}/duplicate"),
            json!({ "name": "Sunday PM" }),
        )
        .await;
    let full = t.get(&format!("/api/shows/{dup}")).await;
    assert_eq!(full["songs"].as_array().unwrap().len(), 3);
    assert_eq!(full["kits"].as_array().unwrap().len(), 4);
    assert_eq!(full["assignments"].as_array().unwrap().len(), 6);
    let dup_conflicts = t.get(&format!("/api/shows/{dup}/conflicts")).await;
    assert!(dup_conflicts["by_song"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn song_ordering() {
    let t = T::new().await;
    let show = t.id("/api/shows", json!({ "name": "S" })).await;
    let a = t
        .id(&format!("/api/shows/{show}/songs"), json!({ "title": "A" }))
        .await;
    let b = t
        .id(&format!("/api/shows/{show}/songs"), json!({ "title": "B" }))
        .await;
    let c = t
        .id(
            &format!("/api/shows/{show}/songs"),
            json!({ "title": "C", "position": 1 }),
        )
        .await;
    let full = t.get(&format!("/api/shows/{show}")).await;
    let order: Vec<i64> = full["songs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_i64().unwrap())
        .collect();
    assert_eq!(order, vec![c, a, b]);

    let (s, _) = t
        .req(
            "POST",
            &format!("/api/shows/{show}/songs/reorder"),
            Some(json!({ "song_ids": [a, b] })),
            true,
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let r = t
        .post(
            &format!("/api/shows/{show}/songs/reorder"),
            json!({ "song_ids": [b, c, a] }),
        )
        .await;
    let order: Vec<i64> = r
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_i64().unwrap())
        .collect();
    assert_eq!(order, vec![b, c, a]);

    let (s, _) = t
        .req("DELETE", &format!("/api/songs/{c}"), None, true)
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let full = t.get(&format!("/api/shows/{show}")).await;
    let pos: Vec<i64> = full["songs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["position"].as_i64().unwrap())
        .collect();
    assert_eq!(pos, vec![1, 2]);
}

#[tokio::test]
async fn checkout_and_return() {
    let t = T::new().await;
    let pack = t.equipment("iem_pack", "EK-1").await;
    let perf = t.id("/api/performers", json!({ "name": "Drummer" })).await;
    let co = t
        .id(
            "/api/checkouts",
            json!({ "equipment_id": pack, "performer_id": perf }),
        )
        .await;
    assert_eq!(
        t.get(&format!("/api/equipment/{pack}")).await["status"],
        "checked_out"
    );

    let (s, _) = t
        .req(
            "POST",
            "/api/checkouts",
            Some(json!({ "equipment_id": pack })),
            true,
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(
        t.get("/api/checkouts?open=true")
            .await
            .as_array()
            .unwrap()
            .len(),
        1
    );

    t.post(
        &format!("/api/checkouts/{co}/return"),
        json!({ "faulty": true }),
    )
    .await;
    assert_eq!(
        t.get(&format!("/api/equipment/{pack}")).await["status"],
        "faulty"
    );
    let (s, _) = t
        .req("POST", &format!("/api/checkouts/{co}/return"), None, true)
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = t
        .req(
            "POST",
            "/api/checkouts",
            Some(json!({ "equipment_id": pack })),
            true,
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "faulty item cannot be checked out");
}

#[tokio::test]
async fn battery_checks_and_status() {
    let t = T::new().await;
    let ch = t.equipment("receiver_channel", "RX1-1").await;
    let hh = t.equipment("handheld_tx", "HH1").await;
    let bp = t.equipment("bodypack_tx", "BP1").await;
    let ch2 = t.equipment("receiver_channel", "RX1-2").await;
    let cap = t.equipment("capsule", "SM58").await;

    let (s, _) = t
        .req(
            "POST",
            "/api/battery-checks",
            Some(json!({ "equipment_id": cap, "result": "ok" })),
            true,
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "capsules have no battery");
    let (s, _) = t
        .req(
            "POST",
            "/api/battery-checks",
            Some(json!({ "equipment_id": hh, "result": "great" })),
            true,
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let show = t.id("/api/shows", json!({ "name": "S" })).await;
    t.post(
        &format!("/api/shows/{show}/kits"),
        json!({ "kind": "mic", "name": "k1", "base_id": ch, "carrier_id": hh, "element_id": cap }),
    )
    .await;
    t.post(
        &format!("/api/shows/{show}/kits"),
        json!({ "kind": "mic", "name": "k2", "base_id": ch2, "carrier_id": bp }),
    )
    .await;

    t.post(
        "/api/battery-checks",
        json!({ "equipment_id": hh, "result": "fresh", "battery_type": "AA lithium" }),
    )
    .await;
    let st = t.get(&format!("/api/shows/{show}/battery-status")).await;
    let st = st.as_array().unwrap();
    assert_eq!(st.len(), 2);
    let by_label = |l: &str| st.iter().find(|r| r["label"] == l).unwrap().clone();
    assert_eq!(by_label("HH1")["due"], "ok");
    assert_eq!(by_label("BP1")["due"], "unchecked");

    t.post(
        "/api/battery-checks",
        json!({ "equipment_id": hh, "result": "low" }),
    )
    .await;
    let st = t.get(&format!("/api/shows/{show}/battery-status")).await;
    assert_eq!(
        st.as_array()
            .unwrap()
            .iter()
            .find(|r| r["label"] == "HH1")
            .unwrap()["due"],
        "low"
    );
}

#[tokio::test]
async fn alert_lifecycle_and_events() {
    let t = T::new().await;
    let a = t
        .id(
            "/api/alerts",
            json!({ "severity": "critical", "title": "HH3 dropout" }),
        )
        .await;
    let b = t
        .id(
            "/api/alerts",
            json!({ "severity": "warning", "title": "IEM 4 crackle" }),
        )
        .await;
    let (s, _) = t
        .req(
            "POST",
            "/api/alerts",
            Some(json!({ "severity": "meh", "title": "x" })),
            true,
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let active = t.get("/api/alerts?state=active").await;
    assert_eq!(active[0]["id"], a, "critical first");

    let acked = t.post(&format!("/api/alerts/{a}/ack"), json!({})).await;
    assert_eq!(acked["state"], "acknowledged");
    assert_eq!(acked["acked_by"], "tester");
    let (s, _) = t
        .req("POST", &format!("/api/alerts/{a}/ack"), None, true)
        .await;
    assert_eq!(s, StatusCode::CONFLICT);

    // Resolve directly from open sets the implicit ack.
    let r = t
        .post(
            &format!("/api/alerts/{b}/resolve"),
            json!({ "note": "reseated cable" }),
        )
        .await;
    assert_eq!(r["state"], "resolved");
    assert_eq!(r["acked_by"], "tester");
    assert_eq!(r["resolution_note"], "reseated cable");

    assert_eq!(
        t.get("/api/alerts?state=active")
            .await
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let events = t.get("/api/events").await;
    let types: Vec<&str> = events
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["type"].as_str().unwrap())
        .collect();
    assert_eq!(
        types,
        vec![
            "alert.opened",
            "alert.opened",
            "alert.acknowledged",
            "alert.resolved"
        ]
    );
    assert_eq!(events[0]["client_id"], "test-client");
    let after = events[1]["seq"].as_i64().unwrap();
    assert_eq!(
        t.get(&format!("/api/events?after={after}"))
            .await
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn assignment_validation() {
    let t = T::new().await;
    let show = t.id("/api/shows", json!({ "name": "S" })).await;
    let other = t.id("/api/shows", json!({ "name": "Other" })).await;
    let perf = t.id("/api/performers", json!({ "name": "P" })).await;
    let iem_ch = t.equipment("iem_tx_channel", "IEM-1").await;
    let k_other = t
        .id(
            &format!("/api/shows/{other}/kits"),
            json!({ "kind": "iem", "name": "x", "base_id": iem_ch }),
        )
        .await;
    let k = t
        .id(
            &format!("/api/shows/{show}/kits"),
            json!({ "kind": "iem", "name": "y", "base_id": iem_ch }),
        )
        .await;

    for (body, why) in [
        (
            json!({ "performer_id": perf, "kind": "iem", "kit_id": k_other }),
            "kit from other show",
        ),
        (
            json!({ "performer_id": perf, "kind": "mic", "kit_id": k }),
            "kind mismatch",
        ),
        (
            json!({ "performer_id": perf, "kind": "iem", "kit_id": null }),
            "default with no kit",
        ),
        (
            json!({ "performer_id": perf, "kind": "banjo", "kit_id": k }),
            "bad kind",
        ),
    ] {
        let (s, v) = t
            .req(
                "POST",
                &format!("/api/shows/{show}/assignments"),
                Some(body),
                true,
            )
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{why}: {v}");
    }
}
