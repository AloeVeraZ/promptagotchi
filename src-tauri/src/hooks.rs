//! Adds / removes Promptagotchi's hooks in ~/.claude/settings.json.
//!
//! - Everything else in the file is left as it was (a backup is written next to it first), and
//!   key order is preserved.
//! - Our entries are recognised by the script name, so install is idempotent and uninstall exact.
//! - Status hooks are `async`, so they never slow Claude down. Only the two "answer from the pet"
//!   hooks are synchronous, and the pet only holds those while you're in a different app.
use serde_json::{json, Map, Value};
use std::path::Path;

const MARK: &str = "promptagotchi-hook";
const STATUS_EVENTS: [&str; 10] = [
    "SessionStart", "SessionEnd", "UserPromptSubmit", "PreToolUse", "PostToolUse",
    "PostToolUseFailure", "SubagentStart", "SubagentStop", "Notification", "Stop",
];

fn cmd(script_ref: &str, mode: &str) -> String {
    // `|| true`: if the script is ever removed, Claude Code just carries on.
    format!("sh {script_ref} {mode} 2>/dev/null || true")
}

/// Copy the hook script into ~/.promptagotchi so settings.json never points inside the app.
pub fn write_hook_script(src: &Path, dest: &Path) -> std::io::Result<()> {
    let body = std::fs::read_to_string(src)?;
    if std::fs::read_to_string(dest).ok().as_deref() != Some(body.as_str()) {
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(dest, &body)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o755))?;
        }
    }
    Ok(())
}

fn read(settings: &Path) -> Result<(Map<String, Value>, Option<String>), String> {
    match std::fs::read_to_string(settings) {
        Ok(raw) if raw.trim().is_empty() => Ok((Map::new(), Some(raw))),
        Ok(raw) => match serde_json::from_str::<Value>(&raw) {
            Ok(Value::Object(m)) => Ok((m, Some(raw))),
            Ok(_) => Err("settings.json isn't a JSON object".into()),
            Err(e) => Err(format!("settings.json isn't valid JSON ({e})")), // never clobber a file we can't read
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((Map::new(), None)),
        Err(e) => Err(e.to_string()),
    }
}

fn is_ours(h: &Value) -> bool {
    h["command"].as_str().map(|c| c.contains(MARK)).unwrap_or(false)
}

fn strip(settings: &mut Map<String, Value>) {
    let Some(Value::Object(hooks)) = settings.get_mut("hooks") else { return };
    let events: Vec<String> = hooks.keys().cloned().collect();
    for ev in events {
        if let Some(Value::Array(groups)) = hooks.get_mut(&ev) {
            for g in groups.iter_mut() {
                if let Some(Value::Array(hs)) = g.get_mut("hooks") {
                    hs.retain(|h| !is_ours(h));
                }
            }
            groups.retain(|g| g["hooks"].as_array().map(|a| !a.is_empty()).unwrap_or(true));
            if groups.is_empty() {
                hooks.remove(&ev);
            }
        }
    }
    if hooks.is_empty() {
        settings.remove("hooks");
    }
}

fn write(settings_path: &Path, settings: &Map<String, Value>, raw: Option<String>) -> Result<(), String> {
    if let Some(dir) = settings_path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    if let Some(raw) = raw {
        let _ = std::fs::write(settings_path.with_extension("json.promptagotchi-backup"), raw);
    }
    let tmp = settings_path.with_extension("json.promptagotchi-tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(settings).unwrap_or_default() + "\n").map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, settings_path).map_err(|e| e.to_string())
}

pub fn install(settings_path: &Path, script_ref: &str, answer: bool) -> Result<(), String> {
    let (mut settings, raw) = read(settings_path)?;
    strip(&mut settings);
    let hooks = settings.entry("hooks").or_insert_with(|| json!({}));
    let hooks = hooks.as_object_mut().ok_or("settings.json \"hooks\" isn't an object")?;
    let add = |hooks: &mut Map<String, Value>, ev: &str, group: Value| {
        let arr = hooks.entry(ev.to_string()).or_insert_with(|| json!([]));
        if let Value::Array(a) = arr {
            a.push(group);
        }
    };
    for ev in STATUS_EVENTS {
        add(hooks, ev, json!({ "hooks": [{ "type": "command", "command": cmd(script_ref, "event"), "async": true, "timeout": 5 }] }));
    }
    if answer {
        add(hooks, "PermissionRequest", json!({ "hooks": [{ "type": "command", "command": cmd(script_ref, "perm"), "timeout": 600 }] }));
        add(hooks, "PreToolUse", json!({ "matcher": "AskUserQuestion", "hooks": [{ "type": "command", "command": cmd(script_ref, "ask"), "timeout": 600 }] }));
    }
    write(settings_path, &settings, raw)
}

pub fn uninstall(settings_path: &Path) -> Result<(), String> {
    let (mut settings, raw) = read(settings_path)?;
    if raw.is_none() {
        return Ok(());
    }
    strip(&mut settings);
    write(settings_path, &settings, raw)
}

/// "none" | "status" (status only) | "full" (status + answering)
pub fn status(settings_path: &Path) -> &'static str {
    let Ok((settings, _)) = read(settings_path) else { return "none" };
    let has = |ev: &str, mode: &str| {
        settings["hooks"][ev].as_array().map(|gs| gs.iter().any(|g| g["hooks"].as_array().map(|hs| hs.iter().any(|h| is_ours(h) && h["command"].as_str().unwrap_or("").contains(&format!(".sh\" {mode}")))).unwrap_or(false))).unwrap_or(false)
    };
    if settings.get("hooks").is_none() || !has("Stop", "event") {
        "none"
    } else if has("PermissionRequest", "perm") {
        "full"
    } else {
        "status"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn install_uninstall_keeps_your_settings_and_is_idempotent() {
        let dir = std::env::temp_dir().join(format!("pg-hooks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("settings.json");
        let mine = json!({"theme":"dark","hooks":{"Stop":[{"hooks":[{"type":"command","command":"say done"}]}]}});
        std::fs::write(&f, mine.to_string()).unwrap();
        let script = "\"$HOME/.promptagotchi/promptagotchi-hook.sh\"";
        install(&f, script, true).unwrap();
        install(&f, script, true).unwrap();
        let j: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        assert_eq!(j["theme"], "dark");
        assert_eq!(j["hooks"]["Stop"].as_array().unwrap().len(), 2);
        assert_eq!(j["hooks"]["Stop"][0]["hooks"][0]["command"], "say done");
        assert_eq!(j["hooks"]["PreToolUse"].as_array().unwrap().len(), 2);
        assert_eq!(status(&f), "full");
        install(&f, script, false).unwrap();
        assert_eq!(status(&f), "status");
        uninstall(&f).unwrap();
        let j: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        assert_eq!(j, mine);
        assert_eq!(status(&f), "none");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn refuses_to_touch_a_broken_settings_file() {
        let dir = std::env::temp_dir().join(format!("pg-hooks-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("settings.json");
        std::fs::write(&f, "{ not json").unwrap();
        assert!(install(&f, "x", true).is_err());
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "{ not json");
        let _ = std::fs::remove_dir_all(dir);
    }
}
