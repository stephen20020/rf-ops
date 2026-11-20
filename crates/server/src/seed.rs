//! Demo data modelled on the venue's real equipment, for trying the UI and for end-to-end tests.

use std::collections::HashMap;

use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::db::{log_event, Actor};

struct Seeder<'a> {
    tx: Transaction<'a, Sqlite>,
    eq: HashMap<String, i64>,
}

impl Seeder<'_> {
    #[allow(clippy::too_many_arguments)]
    async fn equipment(
        &mut self,
        kind: &str,
        make: &str,
        model: &str,
        label: &str,
        parent: Option<&str>,
        channel: Option<i64>,
        ip: Option<&str>,
    ) -> Result<i64, sqlx::Error> {
        let parent_id = parent.map(|p| self.eq[p]);
        let id = sqlx::query(
            "INSERT INTO equipment (kind, make, model, label, parent_id, channel_no, ip_address) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(kind)
        .bind(make)
        .bind(model)
        .bind(label)
        .bind(parent_id)
        .bind(channel)
        .bind(ip)
        .execute(&mut *self.tx)
        .await?
        .last_insert_rowid();
        self.eq.insert(label.to_owned(), id);
        Ok(id)
    }

    async fn simple(
        &mut self,
        kind: &str,
        make: &str,
        model: &str,
        label: &str,
    ) -> Result<i64, sqlx::Error> {
        self.equipment(kind, make, model, label, None, None, None)
            .await
    }
}

/// Seeds demo data if the database has no equipment. Returns true when data was inserted.
pub async fn seed_demo(pool: &SqlitePool) -> Result<bool, sqlx::Error> {
    let existing: Option<i64> = sqlx::query_scalar("SELECT id FROM equipment LIMIT 1")
        .fetch_optional(pool)
        .await?;
    if existing.is_some() {
        return Ok(false);
    }
    let mut s = Seeder {
        tx: pool.begin().await?,
        eq: HashMap::new(),
    };

    // ---- operators ----
    let mut users = HashMap::new();
    for (name, role) in [
        ("Admin", "admin"),
        ("A2 Stage Left", "a2"),
        ("A2 Stage Right", "a2"),
        ("FOH", "foh"),
        ("Viewer", "viewer"),
    ] {
        let id = sqlx::query("INSERT INTO users (name, role) VALUES (?, ?)")
            .bind(name)
            .bind(role)
            .execute(&mut *s.tx)
            .await?
            .last_insert_rowid();
        users.insert(name, id);
    }

    // ---- Shure ULX-D: 3x ULXD4Q = 12 channels ----
    for rx in 1..=3 {
        let label = format!("RX{rx}");
        s.equipment(
            "receiver",
            "Shure",
            "ULXD4Q",
            &label,
            None,
            None,
            Some(&format!("192.168.1.{}", 20 + rx)),
        )
        .await?;
        for ch in 1..=4 {
            s.equipment(
                "receiver_channel",
                "Shure",
                "ULXD4Q",
                &format!("RX{rx}-{ch}"),
                Some(&label),
                Some(ch),
                None,
            )
            .await?;
        }
    }
    for i in 1..=8 {
        s.simple("handheld_tx", "Shure", "ULXD2", &format!("HH{i}"))
            .await?;
    }
    for i in 1..=4 {
        s.simple("bodypack_tx", "Shure", "ULXD1", &format!("BP{i}"))
            .await?;
        s.simple("headset", "Shure", "TH53", &format!("HS{i}"))
            .await?;
    }
    for i in 1..=5 {
        s.simple("capsule", "Shure", "SM58", &format!("SM58-{i}"))
            .await?;
    }
    for i in 1..=2 {
        s.simple("capsule", "Shure", "Beta 58A", &format!("B58-{i}"))
            .await?;
        s.simple("capsule", "Shure", "KSM9", &format!("KSM9-{i}"))
            .await?;
    }

    // ---- Sennheiser IEM: 10x XSW IEM, 3x SR IEM G4 ----
    for i in 1..=10 {
        s.simple(
            "iem_tx_channel",
            "Sennheiser",
            "XSW IEM SR",
            &format!("XSW-{i}"),
        )
        .await?;
        s.simple("iem_pack", "Sennheiser", "XSW IEM EK", &format!("XEK-{i}"))
            .await?;
    }
    for i in 1..=3 {
        s.equipment(
            "iem_tx_channel",
            "Sennheiser",
            "SR IEM G4",
            &format!("G4-{i}"),
            None,
            None,
            Some(&format!("192.168.1.{}", 40 + i)),
        )
        .await?;
        s.simple("iem_pack", "Sennheiser", "EK IEM G4", &format!("G4EK-{i}"))
            .await?;
    }

    // ---- Shure GLX-D guitar ----
    for i in 1..=2 {
        s.simple(
            "guitar_receiver",
            "Shure",
            "GLXD6 Z2",
            &format!("GLXD6-{i}"),
        )
        .await?;
        s.simple("guitar_pack", "Shure", "GLXD1 Z2", &format!("GLXD1-{i}"))
            .await?;
    }
    s.simple("guitar_receiver", "Shure", "GLXD6+ Z3", "GLXD6+-1")
        .await?;
    s.simple("guitar_pack", "Shure", "GLXD1+ Z3", "GLXD1+-1")
        .await?;

    // ---- performers ----
    let sl = users["A2 Stage Left"];
    let sr = users["A2 Stage Right"];
    let mut perf = HashMap::new();
    for (name, role, owner, zone) in [
        ("Jordan", "Worship leader / acoustic", sl, "Center"),
        ("Maya", "Vocal 1", sl, "Stage left"),
        ("Chris", "Vocal 2", sl, "Stage left"),
        ("Priya", "Vocal 3", sl, "Stage left"),
        ("Sam", "Vocal 4", sl, "Stage left"),
        ("Alex", "Keys", sr, "Stage right"),
        ("Taylor", "Electric guitar", sr, "Stage right"),
        ("Morgan", "Bass", sr, "Stage right"),
        ("Riley", "Drums", sr, "Drum riser"),
        ("Pastor Dana", "Message", sl, "Center"),
        ("Casey", "Host / announcements", sl, "Center"),
    ] {
        let id = sqlx::query(
            "INSERT INTO performers (name, role, owner_user_id, zone) VALUES (?, ?, ?, ?)",
        )
        .bind(name)
        .bind(role)
        .bind(owner)
        .bind(zone)
        .execute(&mut *s.tx)
        .await?
        .last_insert_rowid();
        perf.insert(name, id);
    }

    // ---- show ----
    let call = chrono::Utc::now() - chrono::Duration::hours(1);
    let show = sqlx::query(
        "INSERT INTO shows (name, starts_at, call_time, mode) VALUES (?, ?, ?, 'rehearsal')",
    )
    .bind("Sunday Service (demo)")
    .bind((call + chrono::Duration::hours(2)).to_rfc3339())
    .bind(call.to_rfc3339())
    .execute(&mut *s.tx)
    .await?
    .last_insert_rowid();
    let mut song = HashMap::new();
    for (i, title) in [
        "Walk-in / Welcome",
        "Song 1 — Opener",
        "Song 2",
        "Song 3",
        "Song 4 — Ballad",
        "Message",
        "Song 5 — Response",
        "Closing / Announcements",
    ]
    .iter()
    .enumerate()
    {
        let trigger =
            serde_json::json!({ "type": "program_change", "channel": 16, "value": i + 1 })
                .to_string();
        let id = sqlx::query(
            "INSERT INTO songs (show_id, position, title, midi_trigger) VALUES (?, ?, ?, ?)",
        )
        .bind(show)
        .bind(i as i64 + 1)
        .bind(*title)
        .bind(trigger)
        .execute(&mut *s.tx)
        .await?
        .last_insert_rowid();
        song.insert(*title, id);
    }

    // ---- kits ----
    let mut kits = HashMap::new();
    #[allow(clippy::too_many_arguments)]
    async fn kit(
        s: &mut Seeder<'_>,
        kits: &mut HashMap<String, i64>,
        show: i64,
        kind: &str,
        name: &str,
        base: &str,
        carrier: &str,
        element: Option<&str>,
        ch: Option<i64>,
        mix: Option<&str>,
        spare: bool,
    ) -> Result<(), sqlx::Error> {
        let id = sqlx::query(
            "INSERT INTO kits (show_id, kind, name, base_id, carrier_id, element_id, console_channel, console_channel_name, iem_mix, is_spare)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(show)
        .bind(kind)
        .bind(name)
        .bind(s.eq[base])
        .bind(s.eq[carrier])
        .bind(element.map(|e| s.eq[e]))
        .bind(ch)
        .bind(ch.map(|_| name.to_owned()))
        .bind(mix)
        .bind(spare)
        .execute(&mut *s.tx)
        .await?
        .last_insert_rowid();
        kits.insert(name.to_owned(), id);
        Ok(())
    }
    let hh = [
        ("Lead Vox", "RX1-1", "HH1", "SM58-1", 1),
        ("Vox 1", "RX1-2", "HH2", "SM58-2", 2),
        ("Vox 2", "RX1-3", "HH3", "SM58-3", 3),
        ("Vox 3", "RX1-4", "HH4", "B58-1", 4),
        ("Vox 4", "RX2-1", "HH5", "B58-2", 5),
        ("Host HH", "RX2-2", "HH6", "SM58-4", 6),
    ];
    for (name, rx, tx, cap, ch) in hh {
        kit(
            &mut s,
            &mut kits,
            show,
            "mic",
            name,
            rx,
            tx,
            Some(cap),
            Some(ch),
            None,
            false,
        )
        .await?;
    }
    kit(
        &mut s,
        &mut kits,
        show,
        "mic",
        "Lead Vox KSM9",
        "RX1-1",
        "HH1",
        Some("KSM9-1"),
        Some(1),
        None,
        false,
    )
    .await?;
    kit(
        &mut s,
        &mut kits,
        show,
        "mic",
        "Pastor HS",
        "RX2-3",
        "BP1",
        Some("HS1"),
        Some(7),
        None,
        false,
    )
    .await?;
    kit(
        &mut s,
        &mut kits,
        show,
        "mic",
        "Keys talk",
        "RX2-4",
        "BP2",
        Some("HS2"),
        Some(8),
        None,
        false,
    )
    .await?;
    kit(
        &mut s,
        &mut kits,
        show,
        "mic",
        "Spare HH A",
        "RX3-1",
        "HH7",
        Some("SM58-5"),
        Some(9),
        None,
        true,
    )
    .await?;
    kit(
        &mut s,
        &mut kits,
        show,
        "mic",
        "Spare HH B",
        "RX3-2",
        "HH8",
        Some("KSM9-2"),
        Some(10),
        None,
        true,
    )
    .await?;
    kit(
        &mut s,
        &mut kits,
        show,
        "mic",
        "Spare HS",
        "RX3-3",
        "BP3",
        Some("HS3"),
        Some(11),
        None,
        true,
    )
    .await?;
    let iems = [
        ("IEM Jordan", "G4-1", "G4EK-1", "Aux 1"),
        ("IEM Maya", "XSW-1", "XEK-1", "Aux 2"),
        ("IEM Chris", "XSW-2", "XEK-2", "Aux 3"),
        ("IEM Priya", "XSW-3", "XEK-3", "Aux 4"),
        ("IEM Sam", "XSW-4", "XEK-4", "Aux 5"),
        ("IEM Alex", "G4-2", "G4EK-2", "Aux 6"),
        ("IEM Taylor", "XSW-5", "XEK-5", "Aux 7"),
        ("IEM Morgan", "XSW-6", "XEK-6", "Aux 8"),
        ("IEM Riley", "G4-3", "G4EK-3", "Aux 9"),
        ("IEM Spare", "XSW-7", "XEK-7", "Aux 10"),
    ];
    for (name, base, pack, mix) in iems {
        kit(
            &mut s,
            &mut kits,
            show,
            "iem",
            name,
            base,
            pack,
            None,
            None,
            Some(mix),
            name == "IEM Spare",
        )
        .await?;
    }
    kit(
        &mut s,
        &mut kits,
        show,
        "guitar",
        "Acoustic",
        "GLXD6-1",
        "GLXD1-1",
        None,
        Some(20),
        None,
        false,
    )
    .await?;
    kit(
        &mut s,
        &mut kits,
        show,
        "guitar",
        "Electric",
        "GLXD6+-1",
        "GLXD1+-1",
        None,
        Some(21),
        None,
        false,
    )
    .await?;
    kit(
        &mut s,
        &mut kits,
        show,
        "guitar",
        "Guitar spare",
        "GLXD6-2",
        "GLXD1-2",
        None,
        Some(22),
        None,
        true,
    )
    .await?;

    // ---- assignments ----
    async fn assign(
        s: &mut Seeder<'_>,
        show: i64,
        performer: i64,
        kind: &str,
        kit: Option<i64>,
        song: Option<i64>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("INSERT INTO assignments (show_id, performer_id, kind, kit_id, song_id) VALUES (?, ?, ?, ?, ?)")
            .bind(show)
            .bind(performer)
            .bind(kind)
            .bind(kit)
            .bind(song)
            .execute(&mut *s.tx)
            .await?;
        Ok(())
    }
    let defaults = [
        ("Jordan", "mic", "Lead Vox"),
        ("Maya", "mic", "Vox 1"),
        ("Chris", "mic", "Vox 2"),
        ("Priya", "mic", "Vox 3"),
        ("Alex", "mic", "Keys talk"),
        ("Jordan", "iem", "IEM Jordan"),
        ("Maya", "iem", "IEM Maya"),
        ("Chris", "iem", "IEM Chris"),
        ("Priya", "iem", "IEM Priya"),
        ("Sam", "iem", "IEM Sam"),
        ("Alex", "iem", "IEM Alex"),
        ("Taylor", "iem", "IEM Taylor"),
        ("Morgan", "iem", "IEM Morgan"),
        ("Riley", "iem", "IEM Riley"),
        ("Jordan", "guitar", "Acoustic"),
        ("Taylor", "guitar", "Electric"),
    ];
    for (p, kind, k) in defaults {
        assign(&mut s, show, perf[p], kind, Some(kits[k]), None).await?;
    }
    // Casey hosts the welcome and closing on the host handheld.
    for t in ["Walk-in / Welcome", "Closing / Announcements"] {
        assign(
            &mut s,
            show,
            perf["Casey"],
            "mic",
            Some(kits["Host HH"]),
            Some(song[t]),
        )
        .await?;
    }
    // Sam only sings Song 3 and Song 5, on Vox 4.
    for t in ["Song 3", "Song 5 — Response"] {
        assign(
            &mut s,
            show,
            perf["Sam"],
            "mic",
            Some(kits["Vox 4"]),
            Some(song[t]),
        )
        .await?;
    }
    // Ballad: Jordan swaps to the KSM9 capsule on the same handheld.
    assign(
        &mut s,
        show,
        perf["Jordan"],
        "mic",
        Some(kits["Lead Vox KSM9"]),
        Some(song["Song 4 — Ballad"]),
    )
    .await?;
    // Message: vocalists off, pastor on headset.
    assign(
        &mut s,
        show,
        perf["Pastor Dana"],
        "mic",
        Some(kits["Pastor HS"]),
        Some(song["Message"]),
    )
    .await?;
    for p in ["Jordan", "Maya", "Chris", "Priya"] {
        assign(&mut s, show, perf[p], "mic", None, Some(song["Message"])).await?;
    }
    // Closing: Maya is off stage.
    assign(
        &mut s,
        show,
        perf["Maya"],
        "mic",
        None,
        Some(song["Closing / Announcements"]),
    )
    .await?;

    let actor = Actor {
        name: "seed".into(),
        client_id: None,
        role: None,
    };
    log_event(
        &mut s.tx,
        &actor,
        "seed.demo",
        &serde_json::json!({ "show_id": show }),
    )
    .await?;
    s.tx.commit().await?;
    Ok(true)
}
