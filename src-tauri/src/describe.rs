//! Turns raw tool calls into short, human sentences for the session cards.
use crate::util::{clip, rel};
use serde_json::{json, Value};

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("")
}

fn pretty_mcp(name: &str) -> String {
    let parts: Vec<&str> = name.split("__").collect();
    if parts.len() < 3 {
        return name.to_string();
    }
    let mut server = parts[1].to_string();
    let tool = parts[2..].join(" ").replace('_', " ");
    if server.len() >= 20 && server.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        server = "connector".into();
    }
    let server = server.trim_start_matches("ccd_").trim_start_matches("claude-in-").replace('_', " ");
    format!("{server} › {tool}")
}

/// { icon, text, detail } for a tool call.
pub fn describe_tool(name: &str, input: &Value, cwd: Option<&str>) -> Value {
    let (icon, text, detail): (&str, String, String) = match name {
        "Bash" | "PowerShell" => {
            let d = s(input, "description");
            let c = s(input, "command");
            ("run", clip(if d.is_empty() { c } else { d }, 70), if d.is_empty() { String::new() } else { clip(c, 120) })
        }
        "Monitor" => ("run", format!("Watching {}", clip(if s(input, "description").is_empty() { s(input, "command") } else { s(input, "description") }, 60)), String::new()),
        "Read" => ("read", format!("Reading {}", rel(s(input, "file_path"), cwd)), String::new()),
        "Edit" | "MultiEdit" => ("edit", format!("Editing {}", rel(s(input, "file_path"), cwd)), String::new()),
        "Write" => ("edit", format!("Writing {}", rel(s(input, "file_path"), cwd)), String::new()),
        "NotebookEdit" => ("edit", format!("Editing {}", rel(s(input, "notebook_path"), cwd)), String::new()),
        "Grep" => ("search", format!("Searching for “{}”", clip(s(input, "pattern"), 40)), String::new()),
        "Glob" => ("search", format!("Finding {}", clip(s(input, "pattern"), 50)), String::new()),
        "WebFetch" => {
            let u = s(input, "url");
            let host = u.split("://").nth(1).and_then(|r| r.split('/').next()).unwrap_or(u);
            ("web", format!("Reading {}", clip(host, 50)), String::new())
        }
        "WebSearch" => ("web", format!("Searching the web: {}", clip(s(input, "query"), 50)), String::new()),
        "Agent" | "Task" => {
            let d = if !s(input, "description").is_empty() { s(input, "description") } else if !s(input, "subagent_type").is_empty() { s(input, "subagent_type") } else { "working" };
            ("agent", format!("Subagent: {}", clip(d, 60)), String::new())
        }
        "TodoWrite" | "mcp__ccd_turn__update_plan" => ("plan", "Updating the plan".into(), String::new()),
        "AskUserQuestion" => ("ask", "Asking you a question".into(), String::new()),
        "Skill" => ("skill", format!("Using skill {}", clip(if s(input, "skill").is_empty() { s(input, "command") } else { s(input, "skill") }, 40)), String::new()),
        "ExitPlanMode" => ("plan", "Proposing a plan".into(), String::new()),
        n if n.starts_with("mcp__") => ("tool", clip(&pretty_mcp(n), 60), String::new()),
        n => ("tool", clip(if n.is_empty() { "Working" } else { n }, 60), String::new()),
    };
    let mut v = json!({ "icon": icon, "text": text });
    if !detail.is_empty() {
        v["detail"] = json!(detail);
    }
    v
}

/// Title + body for a permission prompt.
pub fn describe_permission(name: &str, input: &Value, cwd: Option<&str>) -> (String, String, bool, String) {
    match name {
        "Bash" | "PowerShell" => ("Run this command?".into(), clip(s(input, "command"), 400), true, clip(s(input, "description"), 120)),
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => {
            let f = if s(input, "file_path").is_empty() { s(input, "notebook_path") } else { s(input, "file_path") };
            (format!("{} {}?", if name == "Write" { "Write" } else { "Edit" }, rel(f, cwd)), String::new(), false, String::new())
        }
        "WebFetch" => ("Fetch this page?".into(), clip(s(input, "url"), 300), true, String::new()),
        "WebSearch" => ("Search the web?".into(), clip(s(input, "query"), 300), false, String::new()),
        "ExitPlanMode" => ("Approve the plan?".into(), clip(s(input, "plan"), 600), false, String::new()),
        _ => {
            let label = if name.starts_with("mcp__") { pretty_mcp(name) } else { name.to_string() };
            (format!("Allow {label}?"), clip(&input.to_string(), 300), true, String::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn describes_common_tools() {
        assert_eq!(describe_tool("Bash", &json!({"command":"npm test","description":"Run tests"}), None)["text"], "Run tests");
        assert_eq!(describe_tool("Read", &json!({"file_path":"/x/a/src/b.ts"}), Some("/x/a"))["text"], "Reading src/b.ts");
        assert_eq!(describe_tool("mcp__ccd_turn__update_plan", &json!({}), None)["text"], "Updating the plan");
        assert_eq!(describe_tool("mcp__github__create_issue", &json!({}), None)["text"], "github › create issue");
        let (t, b, mono, _) = describe_permission("Bash", &json!({"command":"rm -rf build"}), None);
        assert_eq!((t.as_str(), b.as_str(), mono), ("Run this command?", "rm -rf build", true));
    }
}
