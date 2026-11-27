//! "Did that turn end with a question for me?"
//!
//! Claude Code records each session in ~/.claude/projects/<folder>/<sessionId>.jsonl. When a
//! turn ends with a question or next step for you (the desktop app's next-step box, a `wrap_up`
//! call with an `ask`), or with a closing message that asks you something, the pet shows "?"
//! instead of "✓". Only the tail of the file is read, and only the question is kept.
use serde_json::Value;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const TAIL_BYTES: u64 = 768 * 1024;

pub fn transcript_for(projects: &Path, session_id: &str, cwd: Option<&str>) -> Option<PathBuf> {
    let name = format!("{session_id}.jsonl");
    if let Some(c) = cwd {
        let folder: String = c.chars().map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' }).collect();
        let f = projects.join(folder).join(&name);
        if f.exists() {
            return Some(f);
        }
    }
    std::fs::read_dir(projects).ok()?.flatten().map(|e| e.path().join(&name)).find(|f| f.exists())
}

fn read_tail(file: &Path) -> Option<String> {
    let mut f = std::fs::File::open(file).ok()?;
    let size = f.metadata().ok()?.len();
    let len = size.min(TAIL_BYTES);
    f.seek(SeekFrom::Start(size - len)).ok()?;
    let mut buf = Vec::with_capacity(len as usize);
    f.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf).into_owned();
    Some(if len < size { text.split_once('\n').map(|(_, rest)| rest.to_string()).unwrap_or_default() } else { text })
}

/// A real message from you (not a tool result or an internal note) starts a turn.
fn is_your_prompt(d: &Value) -> bool {
    if d["type"] != "user" || d["isMeta"] == true || d["isSidechain"] == true {
        return false;
    }
    match &d["message"]["content"] {
        Value::String(s) => !s.trim().is_empty(),
        Value::Array(items) => items.iter().any(|x| x["type"] == "text" && x["text"].as_str().map(|t| !t.trim().is_empty()).unwrap_or(false)),
        _ => false,
    }
}

/// The last sentence of `text` when it ends with a question mark.
pub fn last_question(text: &str) -> Option<String> {
    let t = text.trim();
    if !t.ends_with('?') {
        return None;
    }
    let mut start = 0;
    let chars: Vec<(usize, char)> = t.char_indices().collect();
    for (i, (idx, c)) in chars.iter().enumerate() {
        let next_ws = chars.get(i + 1).map(|(_, n)| n.is_whitespace()).unwrap_or(false);
        if (*c == '\n') || ((*c == '.' || *c == '!' || *c == '?') && next_ws && i + 1 < chars.len() - 1) {
            start = idx + c.len_utf8();
        }
    }
    let q: String = t[start..].trim().chars().take(300).collect();
    if q.is_empty() { None } else { Some(q) }
}

/// The question your latest turn ended on, if you haven't answered it yet.
pub fn last_turn_ask(projects: &Path, session_id: &str, cwd: Option<&str>) -> Option<String> {
    let file = transcript_for(projects, session_id, cwd)?;
    let tail = read_tail(&file)?;
    let mut closing_text: Option<String> = None;
    for line in tail.lines().rev() {
        if line.is_empty() {
            continue;
        }
        let Ok(d) = serde_json::from_str::<Value>(line) else { continue };
        if is_your_prompt(&d) {
            break; // you've replied since: nothing pending
        }
        if d["type"] != "assistant" || d["isSidechain"] == true {
            continue;
        }
        if let Some(items) = d["message"]["content"].as_array() {
            for c in items.iter().rev() {
                let name = c["name"].as_str().unwrap_or("");
                if c["type"] == "tool_use" && (name == "wrap_up" || name.ends_with("__wrap_up")) {
                    if let Some(ask) = c["input"]["ask"].as_str().filter(|a| !a.trim().is_empty()) {
                        return Some(ask.chars().take(300).collect());
                    }
                }
                if c["type"] == "text" && closing_text.is_none() {
                    closing_text = c["text"].as_str().map(String::from);
                }
            }
        }
    }
    closing_text.and_then(|t| last_question(&t))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn picks_the_closing_question() {
        assert_eq!(last_question("Fixed it. Should I also update the docs?").as_deref(), Some("Should I also update the docs?"));
        assert_eq!(last_question("All done."), None);
        assert_eq!(last_question("Want me to commit?").as_deref(), Some("Want me to commit?"));
    }

    #[test]
    fn reads_next_step_questions_and_skips_answered_ones() {
        let dir = std::env::temp_dir().join(format!("pg-tx-{}", std::process::id()));
        let proj = dir.join("-x-site");
        std::fs::create_dir_all(&proj).unwrap();
        let user = |t: &str| json!({"type":"user","message":{"role":"user","content":t}}).to_string();
        let asst = |c: Value| json!({"type":"assistant","message":{"role":"assistant","content":c}}).to_string();
        let tool_result = json!({"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t","content":"ok"}]}}).to_string();
        let ask = asst(json!([{"type":"tool_use","name":"mcp__ccd_turn__wrap_up","input":{"recap":"r","ask":"Ship it?","blocked":true}}]));
        std::fs::write(proj.join("a.jsonl"), [user("do it"), ask.clone(), tool_result, asst(json!([{"type":"text","text":"Done."}]))].join("\n")).unwrap();
        assert_eq!(last_turn_ask(&dir, "a", Some("/x/site")).as_deref(), Some("Ship it?"));
        std::fs::write(proj.join("b.jsonl"), [ask, user("yes"), asst(json!([{"type":"text","text":"Shipped."}]))].join("\n")).unwrap();
        assert_eq!(last_turn_ask(&dir, "b", Some("/x/site")), None);
        std::fs::write(proj.join("c.jsonl"), [user("hi"), asst(json!([{"type":"text","text":"Fixed the bug. Should I also update the docs?"}]))].join("\n")).unwrap();
        assert_eq!(last_turn_ask(&dir, "c", Some("/x/site")).as_deref(), Some("Should I also update the docs?"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
