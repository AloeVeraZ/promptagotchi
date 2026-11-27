//! Small helpers shared by every module.
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds since the Unix epoch (what the UI and Claude's status files use).
pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Collapse whitespace and cut to `n` characters with an ellipsis.
pub fn clip(s: &str, n: usize) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > n {
        let mut out: String = flat.chars().take(n.saturating_sub(1)).collect();
        out.push('…');
        out
    } else {
        flat
    }
}

/// `file` relative to `cwd` when it's inside it, otherwise just its name.
pub fn rel(file: &str, cwd: Option<&str>) -> String {
    if let Some(c) = cwd {
        let prefix = format!("{}/", c.trim_end_matches('/'));
        if let Some(r) = file.strip_prefix(&prefix) {
            return r.to_string();
        }
    }
    Path::new(file).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| file.to_string())
}

/// Percent-encode one URL path segment.
pub fn enc(seg: &str) -> String {
    let mut out = String::new();
    for b in seg.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

/// Percent-decode one URL path segment.
pub fn dec(seg: &str) -> String {
    let bytes = seg.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() + 0 && i + 2 <= bytes.len() - 1 {
            if let Ok(v) = u8::from_str_radix(&seg[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Copy a folder recursively.
pub fn copy_dir(src: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to: PathBuf = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}

/// `/Users/you/.promptagotchi` → `~/.promptagotchi`
pub fn tilde(p: &Path, home: &Path) -> String {
    match p.strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => p.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clip_and_rel() {
        assert_eq!(clip("a   b\n c", 10), "a b c");
        assert_eq!(clip("abcdefghij", 5), "abcd…");
        assert_eq!(rel("/x/robot/src/a.ts", Some("/x/robot")), "src/a.ts");
        assert_eq!(rel("/elsewhere/b.rs", Some("/x/robot")), "b.rs");
    }
    #[test]
    fn url_segments_round_trip() {
        for s in ["pip", "my pet", "a/b", "ünï"] {
            assert_eq!(dec(&enc(s)), s);
        }
    }
}
