//! Pretend Claude Code sessions, fed through the same path real hooks use, so they show up next
//! to your real sessions: subagent crowds, plans, and the odd permission question. Stopping it
//! removes every pretend session from the list, as if they were never there.
use crate::server::{post_hook, Store};
use crate::sessions::Meta;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const PROJECTS: [(&str, &str, [&str; 3]); 12] = [
    ("flight-tracker", "Live map for flight positions", ["src/map/Layer.tsx", "src/api/adsb.ts", "src/hooks/usePlanes.ts"]),
    ("recipe-api", "Paginate the recipes endpoint", ["app/routes/recipes.py", "app/db/queries.py", "tests/test_recipes.py"]),
    ("pixel-dungeon", "Fog of war for the dungeon", ["src/fov.rs", "src/render/tiles.rs", "src/main.rs"]),
    ("budget-app", "CSV import for bank statements", ["lib/import/csv.dart", "lib/models/txn.dart", "test/import_test.dart"]),
    ("portfolio-site", "Dark mode toggle and theme tokens", ["src/theme.css", "src/components/Toggle.astro", "astro.config.mjs"]),
    ("drone-firmware", "Tune the altitude hold PID", ["src/control/pid.c", "src/sensors/baro.c", "tests/pid_test.c"]),
    ("chat-bot", "Slash commands for the Discord bot", ["bot/commands/index.ts", "bot/commands/remind.ts", "bot/db.ts"]),
    ("weather-widget", "Hourly forecast sparkline", ["Sources/Widget/Forecast.swift", "Sources/Widget/Sparkline.swift", "Tests/ForecastTests.swift"]),
    ("invoice-pdf", "Line-item totals in invoice PDFs", ["src/pdf/table.go", "src/pdf/money.go", "src/pdf/table_test.go"]),
    ("smart-garden", "Soil moisture alerts", ["firmware/moisture.ino", "server/alerts.js", "server/notify.js"]),
    ("typing-game", "Combo multiplier and scoring", ["src/score.ts", "src/ui/Combo.svelte", "src/score.test.ts"]),
    ("photo-sorter", "Group photos by trip", ["sorter/cluster.py", "sorter/exif.py", "tests/test_cluster.py"]),
];
const JOBS: [(&str, &str); 8] = [
    ("Explore", "Find every caller of the old API"), ("Explore", "Map how state flows through the app"),
    ("general-purpose", "Write tests for the edge cases"), ("Explore", "Check how errors are surfaced"),
    ("general-purpose", "Draft the migration script"), ("Plan", "Plan the refactor in small steps"),
    ("Explore", "Look for similar code to reuse"), ("general-purpose", "Benchmark before and after"),
];
const BUILD: [&str; 8] = ["npm test", "npm run build", "cargo test", "pytest -q", "go test ./...", "flutter test", "swift test", "make check"];
const DONE: [&str; 4] = ["Done, and the tests pass.", "Shipped it. One flaky test to look at later.", "Change is in; ready for you to review.", "All green."];

pub struct Simulator {
    stop: Arc<AtomicBool>,
    sessions: Vec<(String, String)>,
    store: Store,
}

fn rnd(lo: u64, hi: u64) -> u64 {
    lo + rand::random::<u64>() % (hi - lo + 1)
}

/// Sleep in small steps; false once the simulator is stopped.
fn nap(stop: &AtomicBool, ms: u64) -> bool {
    let mut left = ms;
    while left > 0 {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        let step = left.min(100);
        std::thread::sleep(Duration::from_millis(step));
        left -= step;
    }
    !stop.load(Ordering::Relaxed)
}

fn meta() -> Meta {
    Meta { term_program: Some("Simulator".into()), ..Default::default() }
}

/// Send one pretend hook event; nothing goes out once the simulator is stopped, so a session
/// can't come back after Stop removed it.
fn post(stop: &AtomicBool, store: &Store, sid: &str, cwd: &str, name: &str, extra: Value) -> Value {
    if stop.load(Ordering::Relaxed) {
        return json!({});
    }
    let mut e = json!({ "session_id": sid, "cwd": cwd, "hook_event_name": name });
    if let Some(m) = extra.as_object() {
        for (k, v) in m { e[k] = v.clone(); }
    }
    post_hook(store, &e, &Meta { mode: if name == "PermissionRequest" { "perm".into() } else { "event".into() }, ..meta() })
}

fn tool(store: &Store, stop: &AtomicBool, sid: &str, cwd: &str, name: &str, input: Value, ms: u64, agent: Option<&str>) -> bool {
    let use_id = format!("{:x}", rand::random::<u64>());
    post(stop, store, sid, cwd, "PreToolUse", json!({ "tool_name": name, "tool_input": input, "tool_use_id": use_id, "agent_id": agent }));
    let ok = nap(stop, ms);
    post(stop, store, sid, cwd, "PostToolUse", json!({ "tool_name": name, "tool_use_id": use_id, "agent_id": agent }));
    ok
}

fn subagent(store: Store, stop: Arc<AtomicBool>, sid: String, cwd: String, files: [&'static str; 3], job: (&'static str, &'static str), n: usize) {
    let id = format!("{sid}-agent-{n}-{:x}", rand::random::<u16>());
    let use_id = format!("{:x}", rand::random::<u64>());
    post(&stop, &store, &sid, &cwd, "PreToolUse", json!({ "tool_name": "Agent", "tool_use_id": use_id, "tool_input": { "description": job.1, "subagent_type": job.0, "prompt": job.1 } }));
    post(&stop, &store, &sid, &cwd, "SubagentStart", json!({ "agent_id": id, "agent_type": job.0 }));
    for _ in 0..rnd(2, 5) {
        let f = files[rnd(0, 2) as usize];
        let (name, input) = match rnd(0, 3) {
            0 => {
                let pattern = ["TODO", "fetch(", "useState", "unwrap()", "retry"][rnd(0, 4) as usize];
                ("Grep", json!({ "pattern": pattern }))
            }
            1 => ("Read", json!({ "file_path": format!("{cwd}/{f}") })),
            2 => ("Glob", json!({ "pattern": "**/*" })),
            _ => ("Bash", json!({ "command": format!("git log --oneline -5 -- {f}"), "description": "Check recent history" })),
        };
        if !tool(&store, &stop, &sid, &cwd, name, input, rnd(900, 2600), Some(&id)) { break; }
    }
    post(&stop, &store, &sid, &cwd, "SubagentStop", json!({ "agent_id": id, "agent_type": job.0 }));
    post(&stop, &store, &sid, &cwd, "PostToolUse", json!({ "tool_name": "Agent", "tool_use_id": use_id }));
}

fn session_loop(store: Store, stop: Arc<AtomicBool>, n: usize, sid: String, cwd: String, questions: bool) {
    let (_, title, files) = PROJECTS[n % PROJECTS.len()];
    if !nap(&stop, rnd(0, 4000)) { return; }
    post(&stop, &store, &sid, &cwd, "SessionStart", json!({ "source": "startup" }));
    let mut turn = 0;
    while !stop.load(Ordering::Relaxed) {
        turn += 1;
        post(&stop, &store, &sid, &cwd, "UserPromptSubmit", json!({ "prompt": format!("{title} (round {turn})"), "session_title": title }));
        let steps = ["Read the code that matters", "Make the change", "Test it", "Tidy up"];
        let items: Vec<Value> = steps.iter().enumerate().map(|(i, c)| json!({ "content": c, "status": if i == 0 { "in_progress" } else { "pending" } })).collect();
        tool(&store, &stop, &sid, &cwd, "mcp__ccd_turn__update_plan", json!({ "items": items }), 300, None);
        for f in &files[..2] {
            if !tool(&store, &stop, &sid, &cwd, "Read", json!({ "file_path": format!("{cwd}/{f}") }), rnd(700, 1800), None) { return; }
        }
        tool(&store, &stop, &sid, &cwd, "mcp__ccd_turn__update_plan", json!({ "advance": true }), 200, None);
        // A crowd of subagents in parallel for some turns.
        let crowd = if rnd(0, 9) < 6 { rnd(2, 6) as usize } else { 0 };
        let mut handles = Vec::new();
        for i in 0..crowd {
            let (store, stop, sid, cwd) = (store.clone(), stop.clone(), sid.clone(), cwd.clone());
            let job = JOBS[(i + n) % JOBS.len()];
            handles.push(std::thread::spawn(move || {
                if nap(&stop, i as u64 * 400) { subagent(store, stop, sid, cwd, files, job, i); }
            }));
        }
        for f in files {
            if !tool(&store, &stop, &sid, &cwd, "Edit", json!({ "file_path": format!("{cwd}/{f}") }), rnd(1200, 3000), None) { break; }
        }
        for h in handles { let _ = h.join(); }
        tool(&store, &stop, &sid, &cwd, "mcp__ccd_turn__update_plan", json!({ "advance": true }), 200, None);
        let cmd = BUILD[rnd(0, 7) as usize];
        if questions && rnd(0, 3) == 0 {
            // Ask permission like a real session would: answer it from the pet, or let it time out.
            post(&stop, &store, &sid, &cwd, "PreToolUse", json!({ "tool_name": "Bash", "tool_input": { "command": cmd, "description": "Run the tests" } }));
            post(&stop, &store, &sid, &cwd, "PermissionRequest", json!({ "tool_name": "Bash", "tool_input": { "command": cmd, "description": "Run the tests" } }));
            if !nap(&stop, rnd(2000, 4000)) { return; }
            post(&stop, &store, &sid, &cwd, "PostToolUse", json!({ "tool_name": "Bash" }));
        } else if !tool(&store, &stop, &sid, &cwd, "Bash", json!({ "command": cmd, "description": "Run the tests" }), rnd(2500, 6000), None) {
            return;
        }
        tool(&store, &stop, &sid, &cwd, "mcp__ccd_turn__update_plan", json!({ "advance": true }), 200, None);
        post(&stop, &store, &sid, &cwd, "Stop", json!({ "last_assistant_message": DONE[rnd(0, 3) as usize] }));
        if !nap(&stop, rnd(6000, 18000)) { return; }
    }
}

impl Simulator {
    pub fn start(store: Store, count: usize, questions: bool) -> Simulator {
        let stop = Arc::new(AtomicBool::new(false));
        let mut sessions = Vec::new();
        for n in 0..count.clamp(1, 30) {
            let (dir, _, _) = PROJECTS[n % PROJECTS.len()];
            let sid = format!("sim-{n}-{dir}");
            let cwd = format!("/Users/you/code/{dir}{}", if n >= PROJECTS.len() { format!("-{}", n / PROJECTS.len()) } else { String::new() });
            sessions.push((sid.clone(), cwd.clone()));
            let (store, stop) = (store.clone(), stop.clone());
            std::thread::spawn(move || session_loop(store, stop, n, sid, cwd, questions));
        }
        Simulator { stop, sessions, store }
    }

    /// Stop, and remove every pretend session (and any prompt one was holding) from the list.
    pub fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        // Let an event that was already on its way land first, then clear them all out.
        std::thread::sleep(Duration::from_millis(300));
        let keys: Vec<String> = self.sessions.iter().map(|(sid, _)| sid.clone()).collect();
        self.store.lock().unwrap().forget(&keys);
    }

    pub fn is_running(&self) -> bool {
        !self.stop.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::SessionStore;
    use std::sync::Mutex;

    fn keys(store: &Store) -> Vec<String> {
        store.lock().unwrap().snapshot().iter().map(|v| v["key"].as_str().unwrap_or("").to_string()).collect()
    }

    #[test]
    fn stopping_removes_every_pretend_session() {
        let store: Store = Arc::new(Mutex::new(SessionStore::new()));
        let real = json!({ "session_id": "real-1", "cwd": "/x/real", "hook_event_name": "UserPromptSubmit", "prompt": "hi" });
        store.lock().unwrap().handle_hook(&real, &Meta::default());
        let sim = Simulator::start(store.clone(), 3, false);
        std::thread::sleep(Duration::from_millis(5000));
        assert_eq!(keys(&store).len(), 4, "pretend sessions show up next to the real one");
        sim.stop();
        assert_eq!(keys(&store), vec!["real-1"]);
        std::thread::sleep(Duration::from_millis(2000));
        assert_eq!(keys(&store), vec!["real-1"], "nothing comes back after Stop");
    }
}
