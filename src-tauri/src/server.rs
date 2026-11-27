//! Tiny localhost HTTP server that the hook script posts to.
//!
//!   POST /v1/hook?mode=event|perm|ask   body: the hook's stdin JSON
//!   → responds with the JSON the hook should print (often {}), possibly after holding the
//!     request while you answer from the pet.
//!   GET /v1/sessions   current state (token required)      GET /v1/ping
//!
//! Security: 127.0.0.1 only, a random per-install token (a custom header, which also means a
//! browser can't post here without a CORS preflight we never answer), and anything carrying an
//! Origin header is rejected.
use crate::paths;
use crate::sessions::{reply_json, Meta, Reply, SessionStore};
use serde_json::{json, Value};
use std::io::Read;
use std::sync::{Arc, Mutex};

pub type Store = Arc<Mutex<SessionStore>>;
pub const DEFAULT_PORT: u16 = 47386;
const MAX_BODY: u64 = 2 * 1024 * 1024;

pub fn ensure_token() -> String {
    let p = paths::get();
    let _ = std::fs::create_dir_all(&p.data_dir);
    if let Ok(t) = std::fs::read_to_string(p.token_file()) {
        let t = t.trim().to_string();
        if t.len() >= 32 {
            return t;
        }
    }
    let t = format!("{:016x}{:016x}{:016x}", rand::random::<u64>(), rand::random::<u64>(), rand::random::<u64>());
    let _ = std::fs::write(p.token_file(), &t);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(p.token_file(), std::fs::Permissions::from_mode(0o600));
    }
    t
}

/// Wait for your answer if the store is holding the prompt; return what the hook prints.
pub fn settle(reply: Reply, store: &Store) -> Value {
    match reply {
        Reply::Now(v) => v,
        Reply::Hold { id, rx, timeout, kind } => match rx.recv_timeout(timeout) {
            Ok(answer) => reply_json(&kind, answer),
            Err(_) => {
                // Not answered in time: hand it back to Claude's own prompt.
                if let Ok(mut s) = store.lock() {
                    s.resolve_question(&id, None, "timeout");
                }
                json!({})
            }
        },
    }
}

/// Feed one hook event in (the server and the simulator both use this).
pub fn post_hook(store: &Store, evt: &Value, meta: &Meta) -> Value {
    let reply = match store.lock() {
        Ok(mut s) => s.handle_hook(evt, meta),
        Err(_) => return json!({}),
    };
    settle(reply, store)
}

pub fn start(store: Store, frontmost: fn() -> Option<String>) -> Result<u16, String> {
    let port = std::env::var("PROMPTAGOTCHI_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT);
    let token = ensure_token();
    let server = tiny_http::Server::http(("127.0.0.1", port)).map_err(|e| {
        if e.to_string().contains("in use") { "port in use (is another copy running?)".to_string() } else { e.to_string() }
    })?;
    let _ = std::fs::write(paths::get().port_file(), port.to_string());
    std::thread::spawn(move || {
        for req in server.incoming_requests() {
            let (store, token) = (store.clone(), token.clone());
            std::thread::spawn(move || handle(req, &store, &token, frontmost));
        }
    });
    Ok(port)
}

fn header(req: &tiny_http::Request, name: &str) -> Option<String> {
    req.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name)).map(|h| h.value.as_str().to_string()).filter(|v| !v.is_empty())
}

fn handle(mut req: tiny_http::Request, store: &Store, token: &str, frontmost: fn() -> Option<String>) {
    let respond = |req: tiny_http::Request, code: u16, body: Value| {
        let resp = tiny_http::Response::from_string(body.to_string())
            .with_status_code(code)
            .with_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
        let _ = req.respond(resp);
    };
    if header(&req, "Origin").is_some() {
        return respond(req, 403, json!({ "error": "forbidden" }));
    }
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    let is_get = *req.method() == tiny_http::Method::Get;
    if is_get && path == "/v1/ping" {
        return respond(req, 200, json!({ "ok": true, "app": "promptagotchi" }));
    }
    if header(&req, "X-Promptagotchi-Token").as_deref() != Some(token) {
        return respond(req, 401, json!({ "error": "bad token" }));
    }
    if is_get && path == "/v1/sessions" {
        let snap = store.lock().map(|s| s.snapshot()).unwrap_or_default();
        return respond(req, 200, json!({ "sessions": snap }));
    }
    if *req.method() != tiny_http::Method::Post || path != "/v1/hook" {
        return respond(req, 404, json!({ "error": "not found" }));
    }
    let mut body = String::new();
    if req.as_reader().take(MAX_BODY).read_to_string(&mut body).is_err() {
        return respond(req, 400, json!({ "error": "bad body" }));
    }
    let Ok(evt) = serde_json::from_str::<Value>(if body.trim().is_empty() { "{}" } else { &body }) else {
        return respond(req, 400, json!({ "error": "bad json" }));
    };
    let mode = query.split('&').find_map(|kv| kv.strip_prefix("mode=")).unwrap_or("event").to_string();
    let mut meta = Meta {
        mode: mode.clone(),
        bundle_id: header(&req, "X-Host-Bundle"),
        term_program: header(&req, "X-Term-Program"),
        iterm_session: header(&req, "X-Iterm-Session"),
        pid: header(&req, "X-Claude-Pid").and_then(|p| p.parse().ok()),
        frontmost: None,
    };
    // Prompts are only held while you're NOT looking at the app that runs the session.
    if mode == "perm" || mode == "ask" {
        meta.frontmost = frontmost();
    }
    let out = post_hook(store, &evt, &meta);
    respond(req, 200, out);
}
