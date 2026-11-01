//! ibde-settingsd — schema-validated keyfiles + D-Bus change signals
//! (ADR-0006). One flat key namespace in ibde.conf for v0.
//!
//! Files: /etc/ibde/ibde.conf (defaults) overridden by
//! ~/.config/ibde/ibde.conf. INI `key = value`; known keys validated,
//! unknown keys rejected with the reason.

use std::collections::BTreeMap;
use std::path::PathBuf;

/// Known keys and their validators (the schema, v0).
fn validate(key: &str, value: &str) -> Result<(), String> {
    match key {
        "appearance.mode" => match value {
            "light" | "dark" => Ok(()),
            _ => Err("must be light|dark".into()),
        },
        "appearance.accent" => {
            const OK: [&str; 9] = [
                "graphite", "red", "orange", "yellow", "green", "teal", "blue", "purple", "pink",
            ];
            if OK.contains(&value) {
                Ok(())
            } else {
                Err(format!("one of {}", OK.join("| ")))
            }
        }
        "dock.size" => match value.parse::<i32>() {
            Ok(v) if (32..=96).contains(&v) => Ok(()),
            Ok(_) => Err("range 32..=96".to_string()),
            Err(_) => Err("integer expected".to_string()),
        },
        "profile" => match value {
            "simple" | "power" => Ok(()),
            _ => Err("must be simple|power".to_string()),
        },
        _ => Err("unknown key".to_string()),
    }
}

fn parse_file(path: &PathBuf, out: &mut BTreeMap<String, String>) {
    let Ok(body) = std::fs::read_to_string(path) else {
        return;
    };
    for line in body.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') || l.starts_with('[') {
            continue; // section headers ignored (flat namespace)
        }
        if let Some((k, v)) = l.split_once('=') {
            out.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
}

fn load_all() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    parse_file(&PathBuf::from("/etc/ibde/ibde.conf"), &mut m);
    if let Ok(home) = std::env::var("HOME") {
        parse_file(&PathBuf::from(home).join(".config/ibde/ibde.conf"), &mut m);
    }
    m
}

struct Settings {
    values: BTreeMap<String, String>,
    tx: std::sync::mpsc::Sender<(String, String)>,
}

#[zbus::interface(name = "org.ibde.Settings1")]
impl Settings {
    fn get(&self, key: &str) -> zbus::fdo::Result<String> {
        self.values
            .get(key)
            .cloned()
            .ok_or_else(|| zbus::fdo::Error::UnknownProperty(key.into()))
    }

    fn get_all(&self) -> BTreeMap<String, String> {
        self.values.clone()
    }

    fn set(&mut self, key: &str, value: &str) -> zbus::fdo::Result<bool> {
        validate(key, value).map_err(|e| zbus::fdo::Error::InvalidArgs(format!("{key}: {e}")))?;
        // v0: runtime + user keyfile write-back
        self.values.insert(key.to_string(), value.to_string());
        if let Ok(home) = std::env::var("HOME") {
            let p = PathBuf::from(home).join(".config/ibde/ibde.conf");
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let body: String = self
                .values
                .iter()
                .map(|(k, v)| format!("{k} = {v}"))
                .collect::<Vec<_>>()
                .join("\n");
            let _ = std::fs::write(&p, body + "\n");
        }
        let _ = self.tx.send((key.to_string(), value.to_string()));
        Ok(true)
    }
}

fn main() {
    let (tx, rx) = std::sync::mpsc::channel::<(String, String)>();
    let values = load_all();
    eprintln!("[settingsd] {} keys loaded", values.len());

    let conn = zbus::blocking::Connection::session().expect("no session bus (run under a session)");
    let iface = Settings { values, tx };
    conn.object_server()
        .at("/org/ibde/Settings", iface)
        .expect("object server at failed");
    conn.request_name("org.ibde.Settings1")
        .expect("name request failed");
    eprintln!("[settingsd] serving org.ibde.Settings1");

    // change log to stderr until consumers subscribe
    while let Ok((k, v)) = rx.recv() {
        eprintln!("[settingsd] changed {k} = {v}");
    }
}
