//! SessionStore: the single source of truth for "what is every Claude Code session doing".
//!
//! Two inputs feed it:
//!   1. Session status files (~/.claude/sessions/<pid>.json): zero setup; title, cwd, busy/idle.
//!   2. Hook events posted by promptagotchi-hook.sh: live tool activity, subagents, plans,
//!      permission prompts and questions (which the pet can answer).
//!
//! Each session ends up in one state:  question > done > working > idle > ended
use crate::describe::{describe_permission, describe_tool};
use crate::transcripts::last_question;
use crate::util::{clip, now_ms};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

const BUSY: [&str; 3] = ["busy", "running", "working"];
const WAITING: [&str; 4] = ["waiting", "blocked", "needs_input", "needs-input"];
const NEEDS_YOU: [&str; 4] = ["permission_prompt", "elicitation_dialog", "elicitation_url_dialog", "agent_needs_input"];
/// Nothing disappears on its own; only closed sessions are capped.
const MAX_ENDED: usize = 40;
/// "busy" for this long with no news (and no hooks saying otherwise) = probably waiting on you.
const STALE_BUSY_MS: i64 = 30 * 60 * 1000;
/// At startup, idle sessions from the last two days are checked for a question you never answered.
const ASK_LOOKBACK_MS: i64 = 48 * 3600 * 1000;

fn is_busy(s: &Option<String>) -> bool {
    s.as_deref().map(|x| BUSY.contains(&x)).unwrap_or(false)
}
fn is_waiting(s: &Option<String>) -> bool {
    s.as_deref().map(|x| WAITING.contains(&x)).unwrap_or(false)
}
fn str_of<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(|x| x.as_str()).filter(|s| !s.is_empty())
}

#[derive(Debug, Clone)]
pub struct Attention {
    pub kind: &'static str, // question | done | error
    pub text: String,
    pub at: i64,
    /// A question Claude asked as its turn ended (informational: answer it in Claude).
    pub ask: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Host {
    pub bundle_id: Option<String>,
    pub term_program: Option<String>,
    pub iterm_session: Option<String>,
}

#[derive(Debug, Clone)]
struct Sub {
    id: String,
    kind: String,
    description: String,
    since: i64,
    activity: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub key: String,
    pub session_id: String,
    pub pid: Option<i64>,
    pub host_session_id: Option<String>,
    pub entrypoint: Option<String>,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub host: Host,
    from_file: bool,
    file_status: Option<String>,
    file_status_at: Option<i64>,
    hook_working: bool,
    hooked: bool,
    last_hook_at: i64,
    activity: Option<Value>,
    subagents: Vec<Sub>,
    plan: Option<Vec<(String, String)>>,
    last_prompt: Option<String>,
    working_since: Option<i64>,
    attention: Option<Attention>,
    ended: bool,
    ended_at: Option<i64>,
    dismissed: bool,
    started_at: i64,
    updated_at: i64,
    pending_ask: Option<String>,
    last_stop_at: Option<i64>,
}

impl Session {
    fn new(id: &str) -> Session {
        let now = now_ms();
        Session {
            key: id.into(), session_id: id.into(), pid: None, host_session_id: None, entrypoint: None, title: None, cwd: None,
            host: Host::default(), from_file: false, file_status: None, file_status_at: None, hook_working: false, hooked: false,
            last_hook_at: 0, activity: None, subagents: Vec::new(), plan: None, last_prompt: None, working_since: None,
            attention: None, ended: false, ended_at: None, dismissed: false, started_at: now, updated_at: now, pending_ask: None, last_stop_at: None,
        }
    }
    fn project(&self) -> String {
        self.cwd.as_deref().and_then(|c| std::path::Path::new(c).file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }
}

struct Question {
    id: String,
    session_key: String,
    created_at: i64,
    expires_at: Option<i64>,
    kind: &'static str, // permission | ask
    title: String,
    body: String,
    mono: bool,
    note: String,
    options: Value,
    questions: Value,
    responder: Option<Sender<Option<Value>>>,
}

#[derive(Debug, Clone, Default)]
pub struct Meta {
    pub mode: String,
    pub bundle_id: Option<String>,
    pub term_program: Option<String>,
    pub iterm_session: Option<String>,
    pub pid: Option<i64>,
    pub frontmost: Option<String>,
}

pub enum HoldKind {
    Permission,
    Ask(Value),
}

/// What the hook should print: right away, or after you answer from the pet.
pub enum Reply {
    Now(Value),
    Hold { id: String, rx: Receiver<Option<Value>>, timeout: Duration, kind: HoldKind },
}

pub enum StoreEvent {
    Change,
    Attention(Value),
    Hook { evt: Value, title: String },
}

type HoldPolicy = Box<dyn Fn(&Session, &Meta) -> (bool, u64) + Send>;
type TurnEnding = Box<dyn Fn(&Session) -> Option<String> + Send>;

pub struct SessionStore {
    sessions: HashMap<String, Session>,
    questions: Vec<Question>,
    ready: bool,
    pub hold_policy: HoldPolicy,
    pub turn_ending: TurnEnding,
    events: Option<Sender<StoreEvent>>,
}

impl Default for SessionStore {
    fn default() -> Self {
        SessionStore::new()
    }
}

impl SessionStore {
    pub fn new() -> SessionStore {
        SessionStore {
            sessions: HashMap::new(),
            questions: Vec::new(),
            ready: false,
            hold_policy: Box::new(|_, _| (false, 0)),
            turn_ending: Box::new(|_| None),
            events: None,
        }
    }

    pub fn set_events(&mut self, tx: Sender<StoreEvent>) {
        self.events = Some(tx);
    }

    fn emit(&self, e: StoreEvent) {
        if let Some(tx) = &self.events {
            let _ = tx.send(e);
        }
    }
    fn changed(&self) {
        self.emit(StoreEvent::Change);
    }
    fn attention_event(&self, key: &str) {
        if let Some(s) = self.sessions.get(key) {
            self.emit(StoreEvent::Attention(self.view(s)));
        }
    }

    fn ensure(&mut self, id: &str) -> &mut Session {
        self.sessions.entry(id.to_string()).or_insert_with(|| Session::new(id))
    }
    fn s(&mut self, key: &str) -> &mut Session {
        self.sessions.get_mut(key).expect("session exists")
    }

    fn touch(&mut self, key: &str) {
        if let Some(s) = self.sessions.get_mut(key) {
            s.updated_at = now_ms();
        }
        self.changed();
    }

    fn set_activity(&mut self, key: &str, mut act: Value) {
        act["at"] = json!(now_ms());
        self.s(key).activity = Some(act);
    }

    /// New activity brings a dismissed or closed session back.
    fn wake(&mut self, key: &str) {
        let s = self.s(key);
        s.dismissed = false;
        s.ended = false;
        s.ended_at = None;
    }

    fn start_working(&mut self, key: &str) {
        self.wake(key);
        let s = self.s(key);
        if s.working_since.is_none() || (!s.hook_working && !is_busy(&s.file_status)) {
            s.working_since = Some(now_ms());
        }
        s.hook_working = true;
        if s.attention.as_ref().map(|a| a.kind != "question").unwrap_or(false) {
            s.attention = None;
        }
    }

    fn has_pending_question(&self, key: &str) -> bool {
        self.questions.iter().any(|q| q.session_key == key)
    }

    /// The session made progress: drop prompts we were only pointing at.
    fn moved_on(&mut self, key: &str) {
        self.questions.retain(|q| !(q.session_key == key && q.responder.is_none()));
        let pending = self.has_pending_question(key);
        let s = self.s(key);
        if s.attention.as_ref().map(|a| a.kind == "question").unwrap_or(false) && !pending {
            s.attention = None;
        }
    }

    fn clear_question(&mut self, key: &str) {
        let s = self.s(key);
        if s.attention.as_ref().map(|a| a.kind == "question").unwrap_or(false) {
            s.attention = None;
        }
        let ids: Vec<String> = self.questions.iter().filter(|q| q.session_key == key).map(|q| q.id.clone()).collect();
        for id in ids {
            self.resolve_question(&id, None, "moved-on");
        }
    }

    /// A turn ended. If it ended on a question or next step for you, that's "needs you".
    fn finish(&mut self, key: &str, shown: String, full: &str) {
        let was_working = {
            let s = self.s(key);
            s.hook_working || is_busy(&s.file_status)
        };
        {
            let s = self.s(key);
            s.hook_working = false;
            s.subagents.clear();
        }
        self.clear_question(key);
        let s = self.s(key);
        let ask = s.pending_ask.take().or_else(|| last_question(full));
        let mut emit = false;
        if let Some(ask) = ask {
            s.attention = Some(Attention { kind: "question", text: ask, at: now_ms(), ask: true });
            emit = true;
        } else if was_working || s.attention.is_none() {
            s.attention = Some(Attention { kind: "done", text: shown, at: now_ms(), ask: false });
            emit = true;
        }
        s.working_since = None;
        if emit {
            self.attention_event(key);
        }
    }

    // ------------------------------------------------------------- status files
    /// `records`: parsed ~/.claude/sessions/*.json whose pid is alive.
    pub fn sync_from_files(&mut self, records: &[Value]) {
        let mut seen = std::collections::HashSet::new();
        for r in records {
            let Some(sid) = str_of(r, "sessionId") else { continue };
            seen.insert(sid.to_string());
            let is_new = !self.sessions.contains_key(sid);
            self.ensure(sid);
            if self.s(sid).ended {
                self.wake(sid);
            }
            let ready = self.ready;
            let prev = self.s(sid).file_status.clone();
            {
                let s = self.s(sid);
                s.from_file = true;
                if let Some(p) = r["pid"].as_i64() { s.pid = Some(p); }
                if let Some(h) = str_of(r, "hostSessionId") { s.host_session_id = Some(h.into()); }
                if let Some(e) = str_of(r, "entrypoint") { s.entrypoint = Some(e.into()); }
                if let Some(c) = str_of(r, "cwd") { s.cwd = Some(c.into()); }
                if let Some(n) = str_of(r, "name") { s.title = Some(n.into()); }
                if let Some(t) = r["startedAt"].as_i64() { s.started_at = t; }
                s.file_status = str_of(r, "status").map(String::from);
                s.file_status_at = r["statusUpdatedAt"].as_i64().or(r["updatedAt"].as_i64());
                if let Some(t) = s.file_status_at {
                    if s.updated_at < t { s.updated_at = t; }
                }
            }
            let now_status = self.s(sid).file_status.clone();
            if prev != now_status {
                let busy = is_busy(&now_status);
                let was_busy = is_busy(&prev);
                if busy && !was_busy {
                    self.wake(sid);
                    let s = self.s(sid);
                    s.working_since = Some(s.file_status_at.unwrap_or_else(now_ms));
                    // Working again: it's no longer "ready", and you've answered what it asked.
                    if s.attention.as_ref().map(|a| a.kind == "done" || (a.kind == "question" && a.ask)).unwrap_or(false) {
                        s.attention = None;
                    }
                }
                if is_waiting(&now_status) {
                    if self.s(sid).attention.as_ref().map(|a| a.kind != "question").unwrap_or(true) {
                        self.wake(sid);
                        self.s(sid).attention = Some(Attention { kind: "question", text: "Waiting for you".into(), at: now_ms(), ask: false });
                        if ready { self.attention_event(sid); }
                    }
                } else if is_waiting(&prev) && !self.has_pending_question(sid) && self.s(sid).attention.as_ref().map(|a| a.kind == "question").unwrap_or(false) {
                    self.s(sid).attention = None;
                }
                if ready && was_busy && !busy && !is_waiting(&now_status) {
                    // Hooks usually beat us to it with Stop; only add the badge if they didn't.
                    if self.s(sid).attention.is_none() {
                        let ask = (self.turn_ending)(self.sessions.get(sid).unwrap());
                        self.s(sid).attention = Some(match ask {
                            Some(a) => Attention { kind: "question", text: a, at: now_ms(), ask: true },
                            None => Attention { kind: "done", text: "Ready for you".into(), at: now_ms(), ask: false },
                        });
                        self.attention_event(sid);
                    }
                    let s = self.s(sid);
                    s.hook_working = false;
                    s.subagents.clear();
                    s.working_since = None;
                }
                let s = self.s(sid);
                if !busy && s.hook_working && s.file_status_at.map(|t| t > s.last_hook_at).unwrap_or(false) {
                    s.hook_working = false; // a Stop hook was missed; trust the newer file
                }
                self.touch(sid);
            }
            if is_new {
                let s = self.s(sid);
                s.updated_at = r["statusUpdatedAt"].as_i64().or(r["updatedAt"].as_i64()).or(r["startedAt"].as_i64()).unwrap_or(s.updated_at);
                // Found idle and recent: did it stop on a question you haven't answered yet?
                if !is_busy(&s.file_status) && !is_waiting(&s.file_status) && now_ms() - s.updated_at < ASK_LOOKBACK_MS {
                    let at = s.updated_at;
                    if let Some(a) = (self.turn_ending)(self.sessions.get(sid).unwrap()) {
                        self.s(sid).attention = Some(Attention { kind: "question", text: a, at, ask: true });
                    }
                }
            }
        }
        // Files that disappeared (window closed, process exited) become "closed" cards.
        let gone: Vec<String> = self.sessions.values().filter(|s| s.from_file && !s.ended && !seen.contains(&s.session_id)).map(|s| s.key.clone()).collect();
        for k in gone {
            self.end(&k);
        }
        self.ready = true;
        self.changed();
    }

    /// The session is gone. Keep its card until you press ×.
    fn end(&mut self, key: &str) {
        let ids: Vec<String> = self.questions.iter().filter(|q| q.session_key == key).map(|q| q.id.clone()).collect();
        for id in ids {
            self.resolve_question(&id, None, "session-ended");
        }
        {
            let s = self.s(key);
            s.ended = true;
            s.ended_at = Some(now_ms());
            s.hook_working = false;
            s.file_status = None;
            s.subagents.clear();
            s.working_since = None;
            if s.attention.as_ref().map(|a| a.kind == "question").unwrap_or(false) {
                s.attention = None;
            }
        }
        let mut ended: Vec<(i64, String)> = self.sessions.values().filter(|s| s.ended).map(|s| (s.ended_at.unwrap_or(0), s.key.clone())).collect();
        ended.sort();
        while ended.len() > MAX_ENDED {
            let (_, k) = ended.remove(0);
            self.sessions.remove(&k);
        }
        self.changed();
    }

    /// Drop these sessions entirely, with no "closed" card (the simulator's, when it stops).
    pub fn forget(&mut self, keys: &[String]) {
        for k in keys {
            if self.sessions.contains_key(k) {
                self.remove(k);
            }
        }
    }

    fn remove(&mut self, key: &str) {
        let ids: Vec<String> = self.questions.iter().filter(|q| q.session_key == key).map(|q| q.id.clone()).collect();
        for id in ids {
            self.resolve_question(&id, None, "session-ended");
        }
        self.sessions.remove(key);
        self.changed();
    }

    /// You pressed ×. Closed sessions go for good; others hide until they do something new.
    pub fn dismiss(&mut self, key: &str) -> bool {
        if !self.sessions.contains_key(key) || self.has_pending_question(key) {
            return false;
        }
        if self.s(key).ended {
            self.remove(key);
            return true;
        }
        let s = self.s(key);
        s.dismissed = true;
        if s.attention.as_ref().map(|a| a.kind != "question" || a.ask).unwrap_or(false) {
            s.attention = None;
        }
        self.changed();
        true
    }

    /// "Clear finished": dismiss every ready and closed session.
    pub fn clear_finished(&mut self) {
        let keys: Vec<String> = self.sessions.values().filter(|s| matches!(self.state(s), "done" | "ended")).map(|s| s.key.clone()).collect();
        for k in keys {
            self.dismiss(&k);
        }
    }

    pub fn mark_seen(&mut self, key: &str) {
        if let Some(s) = self.sessions.get_mut(key) {
            if s.attention.as_ref().map(|a| a.kind == "done" || a.kind == "error").unwrap_or(false) {
                s.attention = None;
                self.changed();
            }
        }
    }

    // ------------------------------------------------------------------- hooks
    /// Handle one hook event. Returns what the hook should print, now or after you answer.
    pub fn handle_hook(&mut self, evt: &Value, meta: &Meta) -> Reply {
        let (Some(name), Some(sid)) = (str_of(evt, "hook_event_name"), str_of(evt, "session_id")) else { return Reply::Now(json!({})) };
        let (name, sid) = (name.to_string(), sid.to_string());
        self.ensure(&sid);
        {
            let s = self.s(&sid);
            let title = s.title.clone().or_else(|| str_of(evt, "session_title").map(String::from)).unwrap_or_else(|| {
                s.cwd.clone().or_else(|| str_of(evt, "cwd").map(String::from)).and_then(|c| std::path::Path::new(&c).file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default()
            });
            let _ = s;
            self.emit(StoreEvent::Hook { evt: evt.clone(), title });
        }
        {
            let s = self.s(&sid);
            s.hooked = true;
            s.last_hook_at = now_ms();
            if s.cwd.is_none() { s.cwd = str_of(evt, "cwd").map(String::from); }
            if let Some(t) = str_of(evt, "session_title") { s.title = Some(t.into()); }
            if meta.bundle_id.is_some() { s.host.bundle_id = meta.bundle_id.clone(); }
            if meta.term_program.is_some() { s.host.term_program = meta.term_program.clone(); }
            if meta.iterm_session.is_some() { s.host.iterm_session = meta.iterm_session.clone(); }
            if s.pid.is_none() { s.pid = meta.pid; }
        }
        let agent = str_of(evt, "agent_id").map(String::from);
        let in_sub = agent.is_some() && name != "SubagentStart" && name != "SubagentStop";
        let tool = str_of(evt, "tool_name").unwrap_or("").to_string();
        let input = evt.get("tool_input").cloned().unwrap_or(Value::Null);
        let mut reply = Reply::Now(json!({}));
        match name.as_str() {
            "SessionStart" => {
                if str_of(evt, "source") == Some("clear") {
                    let s = self.s(&sid);
                    s.plan = None;
                    s.activity = None;
                }
            }
            "SessionEnd" => {
                if !self.s(&sid).from_file { self.end(&sid); } else { self.s(&sid).hook_working = false; }
                self.touch(&sid);
                return Reply::Now(json!({}));
            }
            "UserPromptSubmit" => {
                self.clear_question(&sid);
                let prompt = str_of(evt, "prompt").unwrap_or("");
                {
                    let s = self.s(&sid);
                    s.attention = None;
                    s.last_prompt = Some(clip(prompt, 160));
                    s.pending_ask = None;
                    if s.title.is_none() { s.title = Some(clip(prompt, 60)); }
                    s.working_since = Some(now_ms());
                }
                self.start_working(&sid);
                self.set_activity(&sid, json!({ "icon": "think", "text": "Thinking…" }));
            }
            "PreToolUse" => {
                self.start_working(&sid);
                if tool != "AskUserQuestion" && !in_sub { self.moved_on(&sid); }
                let cwd = self.s(&sid).cwd.clone();
                let act = describe_tool(&tool, &input, cwd.as_deref());
                if in_sub {
                    let text = act["text"].as_str().map(String::from);
                    if let Some(a) = self.s(&sid).subagents.iter_mut().find(|a| Some(&a.id) == agent.as_ref()) { a.activity = text; }
                } else {
                    self.set_activity(&sid, act);
                }
                self.track_plan(&sid, &tool, &input);
                if !in_sub && (tool == "wrap_up" || tool.ends_with("__wrap_up")) {
                    if let Some(a) = str_of(&input, "ask") { self.s(&sid).pending_ask = Some(clip(a, 300)); }
                }
                if tool == "Agent" || tool == "Task" {
                    if let Some(use_id) = str_of(evt, "tool_use_id") {
                        let sub = Sub { id: format!("pending:{use_id}"), kind: str_of(&input, "subagent_type").unwrap_or("agent").into(), description: clip(str_of(&input, "description").unwrap_or("Subagent"), 60), since: now_ms(), activity: None };
                        self.s(&sid).subagents.push(sub);
                    }
                }
                if tool == "AskUserQuestion" && meta.mode == "ask" {
                    reply = self.ask_question(&sid, &input, meta);
                }
            }
            "PostToolUse" | "PostToolUseFailure" => {
                if !in_sub { self.moved_on(&sid); }
                if tool == "Agent" || tool == "Task" {
                    if let Some(use_id) = str_of(evt, "tool_use_id") {
                        let pid = format!("pending:{use_id}");
                        self.s(&sid).subagents.retain(|a| a.id != pid);
                    }
                }
                if !self.s(&sid).hook_working && !in_sub { self.start_working(&sid); }
            }
            "SubagentStart" => {
                let kind = str_of(evt, "agent_type").unwrap_or("agent").to_string();
                let s = self.s(&sid);
                let desc = s.subagents.iter().position(|a| a.id.starts_with("pending:") && a.kind == kind).map(|i| s.subagents.remove(i).description);
                s.subagents.push(Sub { id: agent.clone().unwrap_or_default(), description: desc.unwrap_or_else(|| kind.clone()), kind, since: now_ms(), activity: None });
                self.start_working(&sid);
            }
            "SubagentStop" => {
                self.s(&sid).subagents.retain(|a| Some(&a.id) != agent.as_ref());
            }
            "Notification" => {
                let t = str_of(evt, "notification_type").unwrap_or("");
                let msg = str_of(evt, "message").unwrap_or("");
                let lower = msg.to_lowercase();
                let needs = NEEDS_YOU.contains(&t) || (t.is_empty() && (lower.contains("permission") || lower.contains("needs your") || lower.contains("waiting for your")));
                if needs && self.s(&sid).attention.as_ref().map(|a| a.kind != "question").unwrap_or(true) {
                    self.wake(&sid);
                    self.s(&sid).attention = Some(Attention { kind: "question", text: clip(if msg.is_empty() { "Needs your input" } else { msg }, 120), at: now_ms(), ask: false });
                    self.attention_event(&sid);
                }
            }
            "PermissionRequest" => {
                self.start_working(&sid);
                reply = self.permission_question(&sid, &tool, &input, meta);
            }
            "PermissionDenied" => self.clear_question(&sid),
            "Stop" => {
                if !in_sub {
                    self.s(&sid).last_stop_at = Some(now_ms());
                    let full = str_of(evt, "last_assistant_message").unwrap_or("").to_string();
                    let shown = if full.is_empty() { "Ready for you".to_string() } else { clip(&full, 140) };
                    self.finish(&sid, shown, &full);
                }
            }
            "StopFailure" => {
                let s = self.s(&sid);
                s.hook_working = false;
                s.subagents.clear();
                s.attention = Some(Attention { kind: "error", text: clip(str_of(evt, "error").unwrap_or("Stopped with an error"), 120), at: now_ms(), ask: false });
                self.attention_event(&sid);
            }
            _ => {}
        }
        self.touch(&sid);
        reply
    }

    fn track_plan(&mut self, key: &str, tool: &str, input: &Value) {
        let s = self.s(key);
        if tool == "TodoWrite" {
            if let Some(todos) = input["todos"].as_array() {
                s.plan = Some(todos.iter().map(|t| {
                    let status = t["status"].as_str().unwrap_or("pending").to_string();
                    let content = if status == "in_progress" { t["activeForm"].as_str().or(t["content"].as_str()) } else { t["content"].as_str() };
                    (content.unwrap_or("").to_string(), status)
                }).collect());
            }
        } else if tool == "mcp__ccd_turn__update_plan" {
            if let Some(items) = input["items"].as_array() {
                s.plan = Some(items.iter().map(|t| (t["content"].as_str().unwrap_or("").into(), t["status"].as_str().unwrap_or("pending").into())).collect());
            } else if let Some(plan) = s.plan.as_mut() {
                let cur = plan.iter().position(|(_, st)| st == "in_progress");
                if input["advance"] == true {
                    if let Some(c) = cur { plan[c].1 = "completed".into(); }
                    let from = cur.map(|c| c + 1).unwrap_or(0);
                    if let Some(n) = (from..plan.len()).find(|&i| plan[i].1 == "pending") { plan[n].1 = "in_progress".into(); }
                }
                if let Some(done) = input["done"].as_array() {
                    for d in done.iter().filter_map(|x| x.as_u64()) {
                        if let Some(p) = plan.get_mut(d as usize - 1) { p.1 = "completed".into(); }
                    }
                }
                if let Some(st) = input["start"].as_u64() {
                    if (st as usize) >= 1 && (st as usize) <= plan.len() {
                        for p in plan.iter_mut().filter(|p| p.1 == "in_progress") { p.1 = "pending".into(); }
                        plan[st as usize - 1].1 = "in_progress".into();
                    }
                }
            }
        }
    }

    // --------------------------------------------------------------- questions
    #[allow(clippy::too_many_arguments)]
    fn new_question(&mut self, key: &str, kind: &'static str, title: String, body: String, mono: bool, note: String, options: Value, questions: Value, meta: &Meta) -> Option<(String, Receiver<Option<Value>>, u64)> {
        let (hold, secs) = (self.hold_policy)(self.sessions.get(key).unwrap(), meta);
        let id = format!("{:016x}{:016x}", rand::random::<u64>(), rand::random::<u64>());
        let now = now_ms();
        let mut q = Question { id: id.clone(), session_key: key.into(), created_at: now, expires_at: hold.then(|| now + secs as i64 * 1000), kind, title: title.clone(), body, mono, note, options, questions, responder: None };
        self.wake(key);
        self.s(key).attention = Some(Attention { kind: "question", text: title, at: now, ask: false });
        let out = if hold {
            let (tx, rx) = channel();
            q.responder = Some(tx);
            Some((id, rx, secs))
        } else {
            None
        };
        self.questions.push(q);
        self.attention_event(key);
        out
    }

    fn permission_question(&mut self, key: &str, tool: &str, input: &Value, meta: &Meta) -> Reply {
        if tool == "AskUserQuestion" {
            return Reply::Now(json!({})); // handled by the PreToolUse "ask" hook
        }
        let cwd = self.s(key).cwd.clone();
        let (title, body, mono, note) = describe_permission(tool, input, cwd.as_deref());
        let options = json!([{ "id": "allow", "label": "Allow", "style": "primary" }, { "id": "deny", "label": "Deny", "style": "danger" }]);
        match self.new_question(key, "permission", title, body, mono, note, options, Value::Null, meta) {
            Some((id, rx, secs)) => Reply::Hold { id, rx, timeout: Duration::from_secs(secs), kind: HoldKind::Permission },
            None => Reply::Now(json!({})),
        }
    }

    fn ask_question(&mut self, key: &str, input: &Value, meta: &Meta) -> Reply {
        let Some(qs) = input["questions"].as_array().filter(|q| !q.is_empty()) else { return Reply::Now(json!({})) };
        let questions: Vec<Value> = qs.iter().map(|q| json!({
            "question": q["question"], "header": q["header"].as_str().unwrap_or(""), "multiSelect": q["multiSelect"].as_bool().unwrap_or(false),
            "options": q["options"].as_array().map(|os| os.iter().map(|o| json!({ "label": o["label"], "description": o["description"].as_str().unwrap_or("") })).collect::<Vec<_>>()).unwrap_or_default(),
        })).collect();
        let title = clip(qs[0]["question"].as_str().unwrap_or(""), 200);
        match self.new_question(key, "ask", title, String::new(), false, String::new(), Value::Null, json!(questions), meta) {
            Some((id, rx, secs)) => Reply::Hold { id, rx, timeout: Duration::from_secs(secs), kind: HoldKind::Ask(input.clone()) },
            None => Reply::Now(json!({})),
        }
    }

    /// From the pet: answer = { choice } | { answers }.
    pub fn answer_question(&mut self, id: &str, answer: Value) -> bool {
        self.resolve_question(id, Some(answer), "answered")
    }

    pub fn resolve_question(&mut self, id: &str, answer: Option<Value>, why: &str) -> bool {
        let Some(i) = self.questions.iter().position(|q| q.id == id) else { return false };
        let q = self.questions.remove(i);
        if let Some(tx) = &q.responder {
            let _ = tx.send(answer.clone());
        }
        if self.sessions.contains_key(&q.session_key) {
            if let Some(a) = &answer {
                let s = self.s(&q.session_key);
                if s.attention.as_ref().map(|x| x.kind == "question").unwrap_or(false) { s.attention = None; }
                let text = if a["choice"] == "deny" { "You denied it from the pet" } else { "You answered from the pet" };
                self.set_activity(&q.session_key, json!({ "icon": "ok", "text": text }));
            } else if why != "moved-on" && why != "session-ended" {
                // Back to Claude's own prompt: still needs you until the session moves on.
                self.s(&q.session_key).attention = Some(Attention { kind: "question", text: q.title.clone(), at: now_ms(), ask: false });
            }
        }
        self.changed();
        true
    }

    /// You switched to the app running these sessions: hand their prompts back to Claude.
    pub fn release_for_app(&mut self, bundle: &str) {
        let ids: Vec<String> = self.questions.iter().filter(|q| q.responder.is_some() && self.sessions.get(&q.session_key).and_then(|s| s.host.bundle_id.as_deref()) == Some(bundle)).map(|q| q.id.clone()).collect();
        for id in ids {
            self.resolve_question(&id, None, "host-focused");
        }
    }

    pub fn release_session(&mut self, key: &str) {
        let ids: Vec<String> = self.questions.iter().filter(|q| q.session_key == key).map(|q| q.id.clone()).collect();
        for id in ids {
            self.resolve_question(&id, None, "released");
        }
    }

    pub fn has_held_questions(&self) -> bool {
        self.questions.iter().any(|q| q.responder.is_some())
    }

    // ------------------------------------------------------------------ views
    fn state(&self, s: &Session) -> &'static str {
        if s.ended {
            return "ended";
        }
        if self.has_pending_question(&s.key) || s.attention.as_ref().map(|a| a.kind == "question").unwrap_or(false) {
            return "question";
        }
        if s.hook_working {
            return "working";
        }
        // A "busy" status file only counts if hooks haven't since told us the turn stopped.
        let stopped_since = s.last_stop_at.map(|t| s.file_status_at.map(|f| t >= f).unwrap_or(true)).unwrap_or(false);
        if is_busy(&s.file_status) && !stopped_since {
            return "working";
        }
        if s.attention.as_ref().map(|a| a.kind == "done" || a.kind == "error").unwrap_or(false) {
            return "done";
        }
        "idle"
    }

    pub fn view(&self, s: &Session) -> Value {
        let state = self.state(s);
        let q = self.questions.iter().find(|q| q.session_key == s.key);
        let plan = s.plan.as_ref().and_then(|p| {
            let i = p.iter().position(|(_, st)| st == "in_progress")?;
            Some(json!({ "step": i + 1, "total": p.len(), "text": p[i].0 }))
        });
        let att = s.attention.as_ref().map(|a| json!({ "kind": a.kind, "text": a.text, "at": a.at, "ask": a.ask }));
        let stale = state == "working" && !s.hook_working && now_ms() - s.file_status_at.unwrap_or(0).max(s.last_hook_at) > STALE_BUSY_MS;
        let mut host = Map::new();
        if let Some(b) = &s.host.bundle_id { host.insert("bundleId".into(), json!(b)); }
        if let Some(t) = &s.host.term_program { host.insert("termProgram".into(), json!(t)); }
        if let Some(i) = &s.host.iterm_session { host.insert("itermSession".into(), json!(i)); }
        json!({
            "key": s.key, "sessionId": s.session_id, "pid": s.pid, "hostSessionId": s.host_session_id, "entrypoint": s.entrypoint,
            "title": s.title.clone().unwrap_or_else(|| { let p = s.project(); if p.is_empty() { "Claude session".into() } else { p } }),
            "project": s.project(), "cwd": s.cwd, "state": state, "attention": att,
            "activity": if state == "working" || state == "question" { s.activity.clone() } else { None },
            "lastActivity": s.activity,
            "subagents": s.subagents.iter().map(|a| json!({ "id": a.id, "type": a.kind, "description": a.description, "since": a.since, "activity": a.activity })).collect::<Vec<_>>(),
            "plan": plan, "lastPrompt": s.last_prompt, "stale": stale, "endedAt": s.ended_at, "workingSince": s.working_since,
            "updatedAt": s.updated_at, "hooked": s.hooked, "host": Value::Object(host),
            "question": q.map(|q| json!({
                "id": q.id, "kind": q.kind, "title": q.title, "body": q.body, "mono": q.mono, "note": q.note, "options": q.options,
                "questions": q.questions, "answerable": q.responder.is_some(), "createdAt": q.created_at, "expiresAt": q.expires_at,
            })),
        })
    }

    pub fn snapshot(&self) -> Vec<Value> {
        let prio = |st: &str| match st { "question" => 0, "done" => 1, "working" => 2, "idle" => 3, _ => 4 };
        let mut list: Vec<Value> = self.sessions.values().filter(|s| !s.dismissed).map(|s| self.view(s)).collect();
        // Stable order: a working session keeps its place for its whole turn.
        let since = |v: &Value| -> i64 {
            match v["state"].as_str().unwrap_or("") {
                "working" => v["workingSince"].as_i64().unwrap_or(0),
                "question" | "done" => v["attention"]["at"].as_i64().or(v["updatedAt"].as_i64()).unwrap_or(0),
                "ended" => v["endedAt"].as_i64().unwrap_or(0),
                _ => v["updatedAt"].as_i64().unwrap_or(0),
            }
        };
        list.sort_by(|a, b| {
            prio(a["state"].as_str().unwrap_or("")).cmp(&prio(b["state"].as_str().unwrap_or("")))
                .then(since(b).cmp(&since(a)))
                .then(a["key"].as_str().cmp(&b["key"].as_str()))
        });
        list
    }
}

/// What the hook prints once you've answered from the pet.
pub fn reply_json(kind: &HoldKind, answer: Option<Value>) -> Value {
    let Some(a) = answer else { return json!({}) };
    match kind {
        HoldKind::Permission => match a["choice"].as_str() {
            Some("allow") => json!({ "hookSpecificOutput": { "hookEventName": "PermissionRequest", "decision": { "behavior": "allow" } } }),
            Some("deny") => json!({ "hookSpecificOutput": { "hookEventName": "PermissionRequest", "decision": { "behavior": "deny", "message": "Denied from Promptagotchi." } } }),
            _ => json!({}),
        },
        HoldKind::Ask(input) => {
            if !a["answers"].is_object() {
                return json!({});
            }
            let mut updated = input.clone();
            updated["answers"] = a["answers"].clone();
            json!({ "hookSpecificOutput": { "hookEventName": "PreToolUse", "permissionDecision": "allow", "permissionDecisionReason": "Answered from Promptagotchi", "updatedInput": updated } })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(s: &mut SessionStore, name: &str, extra: Value) -> Reply {
        let mut e = json!({ "session_id": "s1", "cwd": "/x/robot", "hook_event_name": name });
        for (k, v) in extra.as_object().unwrap() { e[k] = v.clone(); }
        s.handle_hook(&e, &Meta { bundle_id: Some("com.apple.Terminal".into()), ..Default::default() })
    }
    fn ev_meta(s: &mut SessionStore, name: &str, extra: Value, meta: Meta) -> Reply {
        let mut e = json!({ "session_id": "s1", "cwd": "/x/robot", "hook_event_name": name });
        for (k, v) in extra.as_object().unwrap() { e[k] = v.clone(); }
        s.handle_hook(&e, &Meta { bundle_id: Some("com.apple.Terminal".into()), ..meta })
    }
    fn first(s: &SessionStore) -> Value { s.snapshot()[0].clone() }
    fn wait(r: Reply, s: &std::sync::Arc<std::sync::Mutex<SessionStore>>) -> Value {
        match r {
            Reply::Now(v) => v,
            Reply::Hold { id, rx, timeout, kind } => match rx.recv_timeout(timeout) {
                Ok(a) => reply_json(&kind, a),
                Err(_) => { s.lock().unwrap().resolve_question(&id, None, "timeout"); json!({}) }
            },
        }
    }

    #[test]
    fn prompt_working_activity_stop_done() {
        let mut s = SessionStore::new();
        ev(&mut s, "UserPromptSubmit", json!({ "prompt": "fix it" }));
        assert_eq!(first(&s)["state"], "working");
        ev(&mut s, "PreToolUse", json!({ "tool_name": "Bash", "tool_input": { "command": "npm test", "description": "Run tests" } }));
        assert_eq!(first(&s)["activity"]["text"], "Run tests");
        ev(&mut s, "Stop", json!({ "last_assistant_message": "All green" }));
        assert_eq!(first(&s)["state"], "done");
        s.mark_seen("s1");
        assert_eq!(first(&s)["state"], "idle");
    }

    #[test]
    fn subagents_and_plans() {
        let mut s = SessionStore::new();
        ev(&mut s, "UserPromptSubmit", json!({ "prompt": "x" }));
        ev(&mut s, "PreToolUse", json!({ "tool_name": "Agent", "tool_use_id": "t1", "tool_input": { "description": "Trace the timer", "subagent_type": "Explore" } }));
        ev(&mut s, "SubagentStart", json!({ "agent_id": "a1", "agent_type": "Explore" }));
        ev(&mut s, "PreToolUse", json!({ "tool_name": "Grep", "tool_input": { "pattern": "tick" }, "agent_id": "a1" }));
        let v = first(&s);
        assert_eq!(v["subagents"].as_array().unwrap().len(), 1);
        assert_eq!(v["subagents"][0]["description"], "Trace the timer");
        ev(&mut s, "SubagentStop", json!({ "agent_id": "a1" }));
        assert_eq!(first(&s)["subagents"].as_array().unwrap().len(), 0);
        ev(&mut s, "PreToolUse", json!({ "tool_name": "mcp__ccd_turn__update_plan", "tool_input": { "items": [{ "content": "A", "status": "in_progress" }, { "content": "B", "status": "pending" }] } }));
        assert_eq!(first(&s)["plan"], json!({ "step": 1, "total": 2, "text": "A" }));
        ev(&mut s, "PreToolUse", json!({ "tool_name": "mcp__ccd_turn__update_plan", "tool_input": { "advance": true } }));
        assert_eq!(first(&s)["plan"], json!({ "step": 2, "total": 2, "text": "B" }));
    }

    #[test]
    fn permission_held_then_allowed_from_the_pet() {
        let store = std::sync::Arc::new(std::sync::Mutex::new(SessionStore::new()));
        store.lock().unwrap().hold_policy = Box::new(|_, _| (true, 5));
        let r = { let mut s = store.lock().unwrap(); ev_meta(&mut s, "PermissionRequest", json!({ "tool_name": "Bash", "tool_input": { "command": "rm -rf build" } }), Meta { mode: "perm".into(), ..Default::default() }) };
        let q = first(&store.lock().unwrap())["question"].clone();
        assert_eq!(q["answerable"], true);
        assert_eq!(q["body"], "rm -rf build");
        store.lock().unwrap().answer_question(q["id"].as_str().unwrap(), json!({ "choice": "allow" }));
        let out = wait(r, &store);
        assert_eq!(out["hookSpecificOutput"]["decision"]["behavior"], "allow");
        assert_eq!(first(&store.lock().unwrap())["state"], "working");
    }

    #[test]
    fn permission_not_held_when_you_are_in_that_app_and_times_out_otherwise() {
        let mut s = SessionStore::new();
        s.hold_policy = Box::new(|sess, meta| (meta.frontmost != sess.host.bundle_id, 0));
        match ev_meta(&mut s, "PermissionRequest", json!({ "tool_name": "Bash", "tool_input": { "command": "ls" } }), Meta { mode: "perm".into(), frontmost: Some("com.apple.Terminal".into()), ..Default::default() }) {
            Reply::Now(v) => assert_eq!(v, json!({})),
            Reply::Hold { .. } => panic!("should not hold"),
        }
        assert_eq!(first(&s)["question"]["answerable"], false);
        ev(&mut s, "PostToolUse", json!({ "tool_name": "Bash" }));
        assert_eq!(first(&s)["state"], "working");
    }

    #[test]
    fn ask_question_answered_from_the_pet() {
        let store = std::sync::Arc::new(std::sync::Mutex::new(SessionStore::new()));
        store.lock().unwrap().hold_policy = Box::new(|_, _| (true, 5));
        let input = json!({ "questions": [{ "question": "Which DB?", "header": "DB", "multiSelect": false, "options": [{ "label": "Postgres" }, { "label": "SQLite" }] }] });
        let r = { let mut s = store.lock().unwrap(); ev_meta(&mut s, "PreToolUse", json!({ "tool_name": "AskUserQuestion", "tool_input": input }), Meta { mode: "ask".into(), ..Default::default() }) };
        { let mut s = store.lock().unwrap(); ev(&mut s, "PreToolUse", json!({ "tool_name": "AskUserQuestion", "tool_input": input })); }
        let q = first(&store.lock().unwrap())["question"].clone();
        assert_eq!(q["kind"], "ask");
        store.lock().unwrap().answer_question(q["id"].as_str().unwrap(), json!({ "answers": { "Which DB?": "SQLite" } }));
        let out = wait(r, &store);
        assert_eq!(out["hookSpecificOutput"]["permissionDecision"], "allow");
        assert_eq!(out["hookSpecificOutput"]["updatedInput"]["answers"], json!({ "Which DB?": "SQLite" }));
        assert_eq!(out["hookSpecificOutput"]["updatedInput"]["questions"], input["questions"]);
    }

    #[test]
    fn turn_ending_on_a_question_shows_needs_you() {
        let mut s = SessionStore::new();
        ev(&mut s, "UserPromptSubmit", json!({ "prompt": "go" }));
        ev(&mut s, "PreToolUse", json!({ "tool_name": "mcp__ccd_turn__wrap_up", "tool_input": { "recap": "Built it.", "ask": "Install Rust?", "blocked": true } }));
        ev(&mut s, "Stop", json!({ "last_assistant_message": "Built it." }));
        let v = first(&s);
        assert_eq!((v["state"].as_str(), v["attention"]["ask"].as_bool(), v["attention"]["text"].as_str()), (Some("question"), Some(true), Some("Install Rust?")));
        ev(&mut s, "UserPromptSubmit", json!({ "prompt": "yes" }));
        ev(&mut s, "Stop", json!({ "last_assistant_message": "Installed." }));
        assert_eq!(first(&s)["state"], "done");
        ev(&mut s, "UserPromptSubmit", json!({ "prompt": "and?" }));
        let long = format!("{} All set. Want me to commit it?", "word ".repeat(60));
        ev(&mut s, "Stop", json!({ "last_assistant_message": long }));
        assert_eq!(first(&s)["attention"]["text"], "Want me to commit it?", "found even past the shortened text");
        assert!(s.dismiss("s1"), "× closes an asking card");
    }

    #[test]
    fn status_files_busy_idle_question_and_closed_cards() {
        let mut s = SessionStore::new();
        s.turn_ending = Box::new(|_| Some("Commit the four new cards?".into()));
        let now = now_ms();
        let rec = json!({ "pid": std::process::id(), "sessionId": "f1", "cwd": "/x/site", "name": "Site", "status": "idle", "statusUpdatedAt": now - 60_000 });
        s.sync_from_files(&[rec.clone()]);
        assert_eq!(first(&s)["state"], "question", "unanswered question found at startup");
        let mut busy = rec.clone(); busy["status"] = json!("busy"); busy["statusUpdatedAt"] = json!(now);
        s.sync_from_files(&[busy]);
        assert_eq!(first(&s)["state"], "working");
        s.turn_ending = Box::new(|_| None);
        let mut idle = rec.clone(); idle["statusUpdatedAt"] = json!(now + 1);
        s.sync_from_files(&[idle]);
        assert_eq!(first(&s)["state"], "done");
        s.sync_from_files(&[]);
        assert_eq!(first(&s)["state"], "ended");
        assert!(s.dismiss("f1"));
        assert!(s.snapshot().is_empty());
    }

    #[test]
    fn stop_from_hooks_beats_a_stale_busy_file_and_order_is_stable() {
        let mut s = SessionStore::new();
        let rec = json!({ "pid": std::process::id(), "sessionId": "s1", "cwd": "/x/a", "name": "A", "status": "busy", "statusUpdatedAt": now_ms() - 6 * 3600_000 });
        s.sync_from_files(&[rec]);
        assert_eq!(first(&s)["stale"], true);
        ev(&mut s, "Stop", json!({ "last_assistant_message": "done" }));
        assert_eq!(first(&s)["state"], "done");

        let mut t = SessionStore::new();
        for id in ["a", "b", "c"] {
            t.handle_hook(&json!({ "session_id": id, "cwd": "/x", "hook_event_name": "UserPromptSubmit", "prompt": "go" }), &Meta::default());
            std::thread::sleep(Duration::from_millis(3));
        }
        let before: Vec<Value> = t.snapshot().iter().map(|v| v["key"].clone()).collect();
        for i in 0..6 {
            let id = ["a", "b", "c"][i % 3];
            t.handle_hook(&json!({ "session_id": id, "cwd": "/x", "hook_event_name": "PreToolUse", "tool_name": "Read", "tool_input": {} }), &Meta::default());
        }
        let after: Vec<Value> = t.snapshot().iter().map(|v| v["key"].clone()).collect();
        assert_eq!(before, after);
    }

    #[test]
    fn finished_sessions_stay_until_x_and_clear_finished() {
        let mut s = SessionStore::new();
        for (id, ev_name) in [("a", "Stop"), ("b", "SessionEnd"), ("c", "UserPromptSubmit")] {
            s.handle_hook(&json!({ "session_id": id, "cwd": format!("/x/{id}"), "hook_event_name": "UserPromptSubmit", "prompt": "x" }), &Meta::default());
            if ev_name != "UserPromptSubmit" {
                s.handle_hook(&json!({ "session_id": id, "cwd": format!("/x/{id}"), "hook_event_name": ev_name }), &Meta::default());
            }
        }
        let mut states: Vec<String> = s.snapshot().iter().map(|v| format!("{}:{}", v["key"].as_str().unwrap(), v["state"].as_str().unwrap())).collect();
        states.sort();
        assert_eq!(states, ["a:done", "b:ended", "c:working"]);
        s.clear_finished();
        assert_eq!(s.snapshot().iter().map(|v| v["key"].clone()).collect::<Vec<_>>(), vec![json!("c")]);
        s.handle_hook(&json!({ "session_id": "a", "cwd": "/x/a", "hook_event_name": "UserPromptSubmit", "prompt": "again" }), &Meta::default());
        assert!(s.snapshot().iter().any(|v| v["key"] == "a"), "new activity brings it back");
    }
}
