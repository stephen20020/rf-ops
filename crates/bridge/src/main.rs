//! rfops-bridge: runs on the MultiTracks Playback computer and forwards MIDI cues to the RF Ops
//! server over the network.
//!
//! By default it creates a virtual MIDI destination called "RF Ops" (macOS/Linux): choose it as
//! Playback's MIDI Out device. With `--port <name>` it listens to an existing input instead (for
//! example "IAC Driver Bus 1", the PlayAUDIO1U, or a loopMIDI port on Windows).

use std::{
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};

use midir::{Ignore, MidiInput, MidiInputConnection};
use rfops_core::midi::MidiMsg;

const VIRTUAL_NAME: &str = "RF Ops";
const HEARTBEAT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq)]
struct Args {
    server: String,
    key: String,
    port: Option<String>,
    name: String,
    list: bool,
}

const USAGE: &str = "\
rfops-bridge — forward MIDI cues from Playback to the RF Ops server

USAGE:
  rfops-bridge --server http://192.168.1.10:8080 --key <bridge key> [--port <name>] [--name <label>]
  rfops-bridge --list

OPTIONS:
  --server <url>   RF Ops server address (or RFOPS_SERVER)
  --key <key>      Bridge key from Admin → MIDI (or RFOPS_BRIDGE_KEY)
  --port <name>    Listen to an existing MIDI input whose name contains this text
                   (default: create a virtual destination called \"RF Ops\")
  --name <label>   Name shown in RF Ops (default: \"Playback bridge\")
  --list           List MIDI inputs and exit
";

fn parse_args(argv: &[String], env: impl Fn(&str) -> Option<String>) -> Result<Args, String> {
    let mut a = Args {
        server: env("RFOPS_SERVER").unwrap_or_default(),
        key: env("RFOPS_BRIDGE_KEY").unwrap_or_default(),
        port: None,
        name: "Playback bridge".into(),
        list: false,
    };
    let mut it = argv.iter();
    while let Some(arg) = it.next() {
        let mut val = |flag: &str| it.next().cloned().ok_or(format!("{flag} needs a value"));
        match arg.as_str() {
            "--server" => a.server = val("--server")?,
            "--key" => a.key = val("--key")?,
            "--port" => a.port = Some(val("--port")?),
            "--name" => a.name = val("--name")?,
            "--list" => a.list = true,
            "-h" | "--help" => return Err(USAGE.into()),
            other => return Err(format!("unknown option {other}\n\n{USAGE}")),
        }
    }
    if !a.list {
        if a.server.is_empty() || a.key.is_empty() {
            return Err(format!("--server and --key are required\n\n{USAGE}"));
        }
        if !a.server.starts_with("http://") && !a.server.starts_with("https://") {
            a.server = format!("http://{}", a.server);
        }
        a.server = a.server.trim_end_matches('/').to_string();
    }
    Ok(a)
}

/// Only cue-type messages are forwarded; clock, sensing and note-offs never leave the Mac.
fn forwardable(bytes: &[u8]) -> bool {
    MidiMsg::parse(bytes).is_some()
}

fn ingest_body(name: &str, port: &str, bytes: &[u8]) -> serde_json::Value {
    serde_json::json!({ "bridge": name, "port": port, "messages": [bytes] })
}

fn new_input() -> Result<MidiInput, String> {
    let mut m = MidiInput::new("rfops-bridge").map_err(|e| format!("MIDI unavailable: {e}"))?;
    m.ignore(Ignore::All);
    Ok(m)
}

fn port_names() -> Vec<String> {
    let Ok(m) = new_input() else { return vec![] };
    m.ports()
        .iter()
        .filter_map(|p| m.port_name(p).ok())
        .collect()
}

struct Listening {
    _conn: MidiInputConnection<()>,
    port: String,
    is_virtual: bool,
}

fn connect(want: Option<&str>, tx: Sender<Vec<u8>>) -> Result<Listening, String> {
    let input = new_input()?;
    let callback = move |_ts: u64, bytes: &[u8], _: &mut ()| {
        if forwardable(bytes) {
            let _ = tx.send(bytes.to_vec());
        }
    };
    match want {
        Some(want) => {
            let needle = want.to_lowercase();
            let ports = input.ports();
            let port = ports
                .iter()
                .find(|p| {
                    input
                        .port_name(p)
                        .is_ok_and(|n| n.to_lowercase().contains(&needle))
                })
                .ok_or(format!("no MIDI input matching \"{want}\""))?
                .clone();
            let name = input.port_name(&port).unwrap_or_default();
            let conn = input
                .connect(&port, "rfops-bridge-in", callback, ())
                .map_err(|e| format!("cannot open {name}: {e}"))?;
            Ok(Listening {
                _conn: conn,
                port: name,
                is_virtual: false,
            })
        }
        None => virtual_port(input, callback),
    }
}

#[cfg(unix)]
fn virtual_port(
    input: MidiInput,
    callback: impl FnMut(u64, &[u8], &mut ()) + Send + 'static,
) -> Result<Listening, String> {
    use midir::os::unix::VirtualInput;
    let conn = input
        .create_virtual(VIRTUAL_NAME, callback, ())
        .map_err(|e| format!("cannot create virtual MIDI port: {e}"))?;
    Ok(Listening {
        _conn: conn,
        port: VIRTUAL_NAME.into(),
        is_virtual: true,
    })
}

#[cfg(not(unix))]
fn virtual_port(
    _input: MidiInput,
    _callback: impl FnMut(u64, &[u8], &mut ()) + Send + 'static,
) -> Result<Listening, String> {
    Err("virtual MIDI ports need macOS or Linux; on Windows use loopMIDI and pass --port".into())
}

fn post(
    agent: &ureq::Agent,
    url: &str,
    key: &str,
    body: &serde_json::Value,
    idem: Option<&str>,
) -> Result<(), String> {
    let mut req = agent
        .post(url)
        .set("x-rfops-bridge-key", key)
        .timeout(Duration::from_secs(3));
    if let Some(k) = idem {
        req = req.set("x-rfops-idempotency-key", k);
    }
    match req.send_json(body.clone()) {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(401, _)) => {
            Err("server rejected the bridge key (Admin → MIDI)".into())
        }
        Err(ureq::Error::Status(code, r)) => Err(format!(
            "server error {code}: {}",
            r.into_string().unwrap_or_default()
        )),
        Err(e) => Err(format!("cannot reach server: {e}")),
    }
}

/// Sends MIDI as it arrives. Retries with the same idempotency key so a cue is never applied twice.
fn sender(args: Args, rx: Receiver<(Vec<u8>, String)>) {
    let agent = ureq::AgentBuilder::new().build();
    let url = format!("{}/api/midi", args.server);
    let mut seq: u64 = 0;
    let boot = format!(
        "{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );
    for (bytes, port) in rx {
        seq += 1;
        let idem = format!("bridge-{boot}-{seq}");
        let label = MidiMsg::parse(&bytes)
            .map(|m| m.describe())
            .unwrap_or_default();
        let body = ingest_body(&args.name, &port, &bytes);
        let started = Instant::now();
        let mut attempt = 0;
        loop {
            attempt += 1;
            match post(&agent, &url, &args.key, &body, Some(&idem)) {
                Ok(()) => {
                    println!("→ {label} sent ({} ms)", started.elapsed().as_millis());
                    break;
                }
                Err(e) if attempt < 4 => {
                    eprintln!("  {label}: {e}; retrying");
                    thread::sleep(Duration::from_millis(250 * attempt));
                }
                Err(e) => {
                    eprintln!("✖ {label} NOT delivered: {e}. Advance the song by hand.");
                    break;
                }
            }
        }
    }
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv, |k| std::env::var(k).ok()) {
        Ok(a) => a,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(if argv.iter().any(|a| a == "-h" || a == "--help") {
                0
            } else {
                2
            });
        }
    };
    if args.list {
        let ports = port_names();
        if ports.is_empty() {
            println!("No MIDI inputs found.");
        }
        for p in ports {
            println!("{p}");
        }
        return;
    }

    println!("RF Ops bridge → {}", args.server);
    let (msg_tx, msg_rx) = mpsc::channel::<Vec<u8>>();
    let (out_tx, out_rx) = mpsc::channel::<(Vec<u8>, String)>();
    {
        let a = args.clone();
        thread::spawn(move || sender(a, out_rx));
    }

    let agent = ureq::AgentBuilder::new().build();
    let hb_url = format!("{}/api/midi/heartbeat", args.server);
    let mut listening: Option<Listening> = None;
    let mut last_err = String::new();
    let mut next_beat = Instant::now();
    let mut server_ok: Option<bool> = None;
    loop {
        // Forward anything received since the last tick.
        while let Ok(bytes) = msg_rx.try_recv() {
            let port = listening
                .as_ref()
                .map(|l| l.port.clone())
                .unwrap_or_default();
            let _ = out_tx.send((bytes, port));
        }
        if Instant::now() >= next_beat {
            next_beat = Instant::now() + HEARTBEAT;
            let ports = port_names();
            // Reconnect if the named port went away (unplugged interface, IAC disabled).
            if let Some(l) = &listening {
                if !l.is_virtual && !ports.contains(&l.port) {
                    eprintln!("MIDI input \"{}\" disappeared; waiting for it…", l.port);
                    listening = None;
                }
            }
            if listening.is_none() {
                match connect(args.port.as_deref(), msg_tx.clone()) {
                    Ok(l) => {
                        if l.is_virtual {
                            println!("Listening on virtual MIDI port \"{}\". In Playback, set MIDI Out to \"{}\".", l.port, l.port);
                        } else {
                            println!("Listening on \"{}\".", l.port);
                        }
                        last_err.clear();
                        listening = Some(l);
                    }
                    Err(e) => {
                        if e != last_err {
                            eprintln!(
                                "{e}. Available inputs: {}",
                                if ports.is_empty() {
                                    "none".into()
                                } else {
                                    ports.join(", ")
                                }
                            );
                            last_err = e;
                        }
                    }
                }
            }
            let body = serde_json::json!({
                "bridge": args.name,
                "port": listening.as_ref().map(|l| l.port.clone()),
                "ports": ports,
            });
            let ok = match post(&agent, &hb_url, &args.key, &body, None) {
                Ok(()) => true,
                Err(e) => {
                    if server_ok != Some(false) {
                        eprintln!("{e}");
                    }
                    false
                }
            };
            if ok && server_ok != Some(true) {
                println!("Connected to RF Ops server.");
            }
            server_ok = Some(ok);
        }
        // Short sleep keeps cue latency low without spinning.
        thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn args_defaults_and_normalisation() {
        let a = parse_args(&argv("--server 192.168.1.10:8080/ --key abc"), |_| None).unwrap();
        assert_eq!(a.server, "http://192.168.1.10:8080");
        assert_eq!(a.key, "abc");
        assert_eq!(a.port, None, "virtual port by default");
        assert_eq!(a.name, "Playback bridge");
    }

    #[test]
    fn args_from_env_and_flags() {
        let env = |k: &str| match k {
            "RFOPS_SERVER" => Some("http://10.0.0.5:8080".into()),
            "RFOPS_BRIDGE_KEY" => Some("k1".into()),
            _ => None,
        };
        let a = parse_args(&argv("--port IAC --name PB-Mac"), env).unwrap();
        assert_eq!(a.server, "http://10.0.0.5:8080");
        assert_eq!(a.port.as_deref(), Some("IAC"));
        assert_eq!(a.name, "PB-Mac");
    }

    #[test]
    fn args_errors() {
        assert!(
            parse_args(&argv("--server x"), |_| None).is_err(),
            "key required"
        );
        assert!(parse_args(&argv("--bogus"), |_| None).is_err());
        assert!(
            parse_args(&argv("--port"), |_| None).is_err(),
            "missing value"
        );
        assert!(
            parse_args(&argv("--list"), |_| None).unwrap().list,
            "list needs no server"
        );
    }

    #[test]
    fn only_cues_are_forwarded() {
        assert!(forwardable(&[0xCF, 5]));
        assert!(forwardable(&[0x9F, 60, 100]));
        assert!(!forwardable(&[0xF8]));
        assert!(!forwardable(&[0x8F, 60, 0]));
        assert!(!forwardable(&[0x9F, 60, 0]));
    }

    #[test]
    fn body_shape() {
        let b = ingest_body("PB", "RF Ops", &[0xCF, 5]);
        assert_eq!(
            b,
            serde_json::json!({ "bridge": "PB", "port": "RF Ops", "messages": [[207, 5]] })
        );
    }
}
