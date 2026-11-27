//! "Take me to that session."
//!
//!  - Claude desktop app (Code tab):  claude://code/continue?session=local_…  opens that session.
//!  - VS Code extension:               vscode://anthropic.claude-code/open?session=<id>
//!  - Terminal.app / iTerm2:            AppleScript selects the tab whose tty runs that claude.
//!  - Anything else:                    bring the hosting app to the front.
//!
//! The host app is found by walking up the process tree from the session's pid.
use serde_json::Value;
use std::collections::HashMap;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn frontmost_bundle_id() -> Option<String> {
    let asn = run("lsappinfo", &["front"])?.trim().to_string();
    if asn.is_empty() {
        return None;
    }
    let info = run("lsappinfo", &["info", "-only", "bundleid", &asn])?;
    let start = info.find("bundleID=\"").map(|i| i + 10).or_else(|| info.find("\"CFBundleIdentifier\"=\"").map(|i| i + 22))?;
    let rest = &info[start..];
    Some(rest[..rest.find('"')?].to_string())
}

#[derive(Clone, Default)]
struct HostApp {
    tty: Option<String>,
    app_path: Option<String>,
}

fn host_of(pid: i64) -> HostApp {
    static CACHE: OnceLock<Mutex<HashMap<i64, HostApp>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(h) = cache.lock().ok().and_then(|c| c.get(&pid).cloned()) {
        return h;
    }
    let mut out = HostApp::default();
    let mut cur = pid;
    for _ in 0..16 {
        if cur <= 1 {
            break;
        }
        let Some(line) = run("ps", &["-o", "ppid=,tty=,comm=", "-p", &cur.to_string()]) else { break };
        let line = line.trim();
        let mut parts = line.splitn(3, char::is_whitespace).filter(|s| !s.is_empty());
        let (Some(ppid), Some(tty)) = (parts.next(), parts.next()) else { break };
        let comm = line.splitn(2, tty).nth(1).unwrap_or("").trim();
        if out.tty.is_none() && tty != "??" {
            out.tty = Some(tty.to_string());
        }
        if let Some(i) = comm.find(".app/") {
            let app = &comm[..i + 4];
            let exe = comm.rsplit('/').next().unwrap_or("");
            // Skip Claude Code's own bundled binary (…/claude-code/<ver>/…/claude.app/Contents/MacOS/claude).
            if exe != "claude" && !app.contains("/claude-code/") {
                out.app_path = Some(app.to_string());
                break;
            }
        }
        cur = ppid.parse().unwrap_or(0);
    }
    if let Ok(mut c) = cache.lock() {
        c.insert(pid, out.clone());
    }
    out
}

fn script_terminal(tty: &str) -> String {
    format!(r#"tell application "Terminal"
  repeat with w in windows
    repeat with t in tabs of w
      if tty of t is "/dev/{tty}" then
        set selected of t to true
        set index of w to 1
        activate
        return "ok"
      end if
    end repeat
  end repeat
  activate
end tell"#)
}

fn script_iterm(tty: &str) -> String {
    format!(r#"tell application "iTerm2"
  repeat with w in windows
    repeat with t in tabs of w
      repeat with s in sessions of t
        if tty of s is "/dev/{tty}" then
          select w
          tell t to select
          tell s to select
          activate
          return "ok"
        end if
      end repeat
    end repeat
  end repeat
  activate
end tell"#)
}

pub fn open_url(url: &str) -> bool {
    Command::new("open").arg(url).status().map(|s| s.success()).unwrap_or(false)
}

pub fn focus_session(v: &Value) -> bool {
    let host_session = v["hostSessionId"].as_str().unwrap_or("");
    let valid_local = host_session.starts_with("local_") && host_session.len() <= 70 && host_session[6..].chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if valid_local {
        return open_url(&format!("claude://code/continue?session={host_session}"));
    }
    let entry = v["entrypoint"].as_str().unwrap_or("");
    let sid = v["sessionId"].as_str().unwrap_or("");
    if entry.contains("vscode") && !sid.is_empty() {
        return open_url(&format!("vscode://anthropic.claude-code/open?session={}", crate::util::enc(sid)));
    }
    let h = v["pid"].as_i64().map(host_of).unwrap_or_default();
    let tty = h.tty.filter(|t| t.starts_with("tty") && t[3..].trim_start_matches('s').chars().all(|c| c.is_ascii_digit()));
    let app = h.app_path.unwrap_or_default();
    let bundle = v["host"]["bundleId"].as_str().unwrap_or("");
    if let Some(t) = &tty {
        if bundle == "com.apple.Terminal" || app.ends_with("/Terminal.app") {
            return run("osascript", &["-e", &script_terminal(t)]).is_some();
        }
        if bundle == "com.googlecode.iterm2" || app.contains("iTerm") {
            return run("osascript", &["-e", &script_iterm(t)]).is_some();
        }
    }
    if !app.is_empty() {
        return Command::new("open").args(["-a", &app]).status().map(|s| s.success()).unwrap_or(false);
    }
    if !bundle.is_empty() && bundle.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
        return Command::new("open").args(["-b", bundle]).status().map(|s| s.success()).unwrap_or(false);
    }
    if entry == "claude-desktop" {
        return Command::new("open").args(["-b", "com.anthropic.claudefordesktop"]).status().map(|s| s.success()).unwrap_or(false);
    }
    false
}
