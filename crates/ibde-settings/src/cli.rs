//! ibde-settings — CLI for org.ibde.Settings1 (ADR-0006):
//!   ibde-settings get KEY | set KEY VALUE | list | monitor

use zbus::blocking::Connection;

fn main() {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_default();
    let conn = Connection::session().expect("no session bus");
    let proxy = zbus::blocking::Proxy::new(
        &conn,
        "org.ibde.Settings1",
        "/org/ibde/Settings",
        "org.ibde.Settings1",
    )
    .expect("settingsd not running?");

    match cmd.as_str() {
        "get" => {
            let key = args.next().expect("get KEY");
            match proxy.call::<String, _>("Get", &(&key,)) {
                Ok(v) => println!("{v}"),
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }
        "set" => {
            let key = args.next().expect("set KEY VALUE");
            let value = args.next().expect("set KEY VALUE");
            match proxy.call::<bool, _>("Set", &(&key, &value)) {
                Ok(_) => println!("ok"),
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }
        "list" => {
            use std::collections::BTreeMap;
            let all: BTreeMap<String, String> = proxy.call("GetAll", &()).unwrap();
            for (k, v) in all {
                println!("{k} = {v}");
            }
        }
        "monitor" => {
            // v0: poll GetAll and print diffs (signal subscription lands
            // with the settings-app wiring)
            use std::collections::BTreeMap;
            let mut last: BTreeMap<String, String> = proxy.call("GetAll", &()).unwrap();
            println!("(monitoring {} keys — Ctrl-C to stop)", last.len());
            loop {
                std::thread::sleep(std::time::Duration::from_millis(500));
                let now: BTreeMap<String, String> = proxy.call("GetAll", &()).unwrap();
                for (k, v) in &now {
                    if last.get(k) != Some(v) {
                        println!("{k} = {v}");
                    }
                }
                last = now;
            }
        }
        other => {
            eprintln!(
                "usage: ibde-settings get KEY | set KEY VALUE | list | monitor (got {other:?})"
            );
            std::process::exit(1);
        }
    }
}
