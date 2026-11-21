//! `rfops`: build shows from files.
//!
//! ```text
//! rfops shows                              list shows
//! rfops show check  FILE.yaml              check a show file (changes nothing)
//! rfops show import FILE.yaml [--dry-run]  create or update the show
//! rfops show export "Show name" [-o FILE]  write a show out as a file
//! rfops show reset  "Show name"            clear a rehearsal/test run (the plan is kept)
//! ```
//!
//! Server: `--server URL` or `RFOPS_SERVER` (default http://localhost:8080).
//! Name for the log: `--name NAME` or `RFOPS_NAME` (default "rfops cli").
//! Exit code 1 when the file has errors, 2 for usage or connection problems.

use std::process::ExitCode;

use serde_json::Value;

struct Opts {
    server: String,
    name: String,
    args: Vec<String>,
}

fn opts() -> Opts {
    let mut server = std::env::var("RFOPS_SERVER").unwrap_or_else(|_| "http://localhost:8080".into());
    let mut name = std::env::var("RFOPS_NAME").unwrap_or_else(|_| "rfops cli".into());
    let mut args = Vec::new();
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--server" => server = it.next().unwrap_or(server),
            "--name" => name = it.next().unwrap_or(name),
            _ => args.push(a),
        }
    }
    Opts {
        server: server.trim_end_matches('/').to_string(),
        name,
        args,
    }
}

const USAGE: &str = "usage:
  rfops shows
  rfops show check  FILE.yaml
  rfops show import FILE.yaml [--dry-run]
  rfops show export \"Show name\" [-o FILE.yaml]
  rfops show reset  \"Show name\"
options: --server URL (or RFOPS_SERVER), --name NAME (or RFOPS_NAME)";

fn fail(msg: &str) -> ExitCode {
    eprintln!("{msg}");
    ExitCode::from(2)
}

fn header_name(n: &str) -> String {
    // HTTP header values must be ASCII; the server decodes %XX.
    n.bytes()
        .map(|b| if b.is_ascii_graphic() || b == b' ' { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

fn post_yaml(o: &Opts, path: &str, body: &str) -> Result<(u16, Value), String> {
    let r = ureq::post(&format!("{}{path}", o.server))
        .set("content-type", "text/yaml")
        .set("x-rfops-actor", &header_name(&o.name))
        .set("x-rfops-client", "rfops-cli")
        .send_string(body);
    match r {
        Ok(resp) => Ok((resp.status(), resp.into_json().unwrap_or(Value::Null))),
        Err(ureq::Error::Status(code, resp)) => Ok((code, resp.into_json().unwrap_or(Value::Null))),
        Err(e) => Err(format!("can't reach RF Ops at {}: {e}", o.server)),
    }
}

fn get(o: &Opts, path: &str) -> Result<ureq::Response, String> {
    ureq::get(&format!("{}{path}", o.server))
        .set("x-rfops-actor", &header_name(&o.name))
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(code, r) => format!("RF Ops answered {code}: {}", r.into_string().unwrap_or_default()),
            e => format!("can't reach RF Ops at {}: {e}", o.server),
        })
}

/// Print errors and warnings; returns true if there were errors.
fn report(v: &Value) -> bool {
    let list = |k: &str| v[k].as_array().cloned().unwrap_or_default();
    let (errors, warnings) = (list("errors"), list("warnings"));
    for e in &errors {
        println!("✗ {}: {}", e["at"].as_str().unwrap_or(""), e["message"].as_str().unwrap_or(""));
    }
    for w in &warnings {
        println!("! {}: {}", w["at"].as_str().unwrap_or(""), w["message"].as_str().unwrap_or(""));
    }
    if errors.is_empty() {
        println!("✓ no errors{}", if warnings.is_empty() { String::new() } else { format!(" ({} warning(s))", warnings.len()) });
    } else {
        println!("{} error(s): nothing was changed.", errors.len());
    }
    !errors.is_empty()
}

fn shows(o: &Opts) -> Result<Vec<Value>, String> {
    get(o, "/api/shows")?
        .into_json::<Vec<Value>>()
        .map_err(|e| e.to_string())
}

fn find_show(o: &Opts, which: &str) -> Result<i64, String> {
    if let Ok(id) = which.parse::<i64>() {
        return Ok(id);
    }
    shows(o)?
        .iter()
        .find(|s| s["name"].as_str() == Some(which))
        .and_then(|s| s["id"].as_i64())
        .ok_or_else(|| format!("no show called \"{which}\" (try: rfops shows)"))
}

fn main() -> ExitCode {
    let o = opts();
    let a: Vec<&str> = o.args.iter().map(String::as_str).collect();
    match a.as_slice() {
        ["shows"] => match shows(&o) {
            Ok(list) => {
                for s in list {
                    println!("{:>4}  {}", s["id"], s["name"].as_str().unwrap_or(""));
                }
                ExitCode::SUCCESS
            }
            Err(e) => fail(&e),
        },
        ["show", cmd @ ("check" | "import"), file, rest @ ..] => {
            let body = match std::fs::read_to_string(file) {
                Ok(b) => b,
                Err(e) => return fail(&format!("can't read {file}: {e}")),
            };
            let dry = rest.contains(&"--dry-run");
            let path = match (*cmd, dry) {
                ("check", _) => "/api/showfile/check".to_string(),
                (_, true) => "/api/showfile/import?dry_run=true".to_string(),
                _ => "/api/showfile/import".to_string(),
            };
            match post_yaml(&o, &path, &body) {
                Ok((code, v)) => {
                    let had_errors = report(&v);
                    if *cmd == "import" && !dry && !had_errors {
                        let verb = if code == 201 { "Created" } else { "Updated" };
                        println!("{verb} show #{} from {file}.", v["show_id"]);
                    }
                    if had_errors { ExitCode::from(1) } else { ExitCode::SUCCESS }
                }
                Err(e) => fail(&e),
            }
        }
        ["show", "reset", which] => {
            let id = match find_show(&o, which) {
                Ok(id) => id,
                Err(e) => return fail(&e),
            };
            let r = ureq::post(&format!("{}/api/shows/{id}/reset-run", o.server))
                .set("x-rfops-actor", &header_name(&o.name))
                .call();
            match r {
                Ok(resp) => {
                    let v: Value = resp.into_json().unwrap_or(Value::Null);
                    println!("Reset \"{which}\": run cleared, {} device(s) back to stock. The plan is unchanged.", v["devices_to_stock"]);
                    ExitCode::SUCCESS
                }
                Err(e) => fail(&format!("reset failed: {e}")),
            }
        }
        ["show", "export", which, rest @ ..] => {
            let id = match which.parse::<i64>() {
                Ok(id) => id,
                Err(_) => match shows(&o) {
                    Ok(list) => match list.iter().find(|s| s["name"].as_str() == Some(which)) {
                        Some(s) => s["id"].as_i64().unwrap_or(0),
                        None => return fail(&format!("no show called \"{which}\" (try: rfops shows)")),
                    },
                    Err(e) => return fail(&e),
                },
            };
            let yaml = match get(&o, &format!("/api/shows/{id}/showfile")).and_then(|r| r.into_string().map_err(|e| e.to_string())) {
                Ok(y) => y,
                Err(e) => return fail(&e),
            };
            match rest {
                ["-o", out] => match std::fs::write(out, yaml) {
                    Ok(()) => {
                        println!("Wrote {out}.");
                        ExitCode::SUCCESS
                    }
                    Err(e) => fail(&format!("can't write {out}: {e}")),
                },
                _ => {
                    print!("{yaml}");
                    ExitCode::SUCCESS
                }
            }
        }
        _ => fail(USAGE),
    }
}
