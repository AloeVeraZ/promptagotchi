//! Watches ~/.claude/sessions/<pid>.json. Claude Code (CLI and the desktop app's Code tab) keeps
//! one small status file per running session: pid, sessionId, cwd, title ("name"), status.
//! This is what lets Promptagotchi work with zero setup. The *.key files next to them are private
//! and never touched.
use crate::paths;
use crate::server::Store;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

fn alive(pid: i64) -> bool {
    if pid <= 0 {
        return false;
    }
    // kill(pid, 0) checks the process exists without signalling it.
    let r = unsafe { libc::kill(pid as libc::pid_t, 0) };
    r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

pub fn spawn(store: Store) {
    std::thread::spawn(move || {
        let mut cache: HashMap<PathBuf, (SystemTime, Value)> = HashMap::new();
        loop {
            tick(&store, &mut cache);
            std::thread::sleep(Duration::from_millis(1000));
        }
    });
}

fn tick(store: &Store, cache: &mut HashMap<PathBuf, (SystemTime, Value)>) {
    let dir = paths::get().claude_sessions();
    let mut records = Vec::new();
    let mut live = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let file = e.path();
            if file.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            let Ok(mtime) = e.metadata().and_then(|m| m.modified()) else { continue };
            live.push(file.clone());
            let fresh = cache.get(&file).map(|(t, _)| *t != mtime).unwrap_or(true);
            if fresh {
                match std::fs::read_to_string(&file).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()) {
                    Some(v) => { cache.insert(file.clone(), (mtime, v)); }
                    None => continue, // mid-write; next tick gets it
                }
            }
            let rec = &cache[&file].1;
            let pid = rec["pid"].as_i64().or_else(|| file.file_stem().and_then(|s| s.to_str()?.parse().ok())).unwrap_or(0);
            if alive(pid) {
                records.push(rec.clone());
            }
        }
    }
    cache.retain(|k, _| live.contains(k));
    if let Ok(mut s) = store.lock() {
        s.sync_from_files(&records);
    }
}
