//! Your settings, kept in ~/.promptagotchi/config.json.
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Place {
    /// The screen it was on (its name); missing = the main screen.
    #[serde(default)]
    pub display: Option<String>,
    /// Where the pet's feet are, as a share of that screen's work area.
    pub fx: f64,
    pub fy: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    pub pet: String,
    /// small | medium | large | huge, relative to the screen it's on
    pub size: String,
    pub place: Option<Place>,
    /// Hold permission prompts / questions so you can answer from the pet.
    pub answer_from_pet: bool,
    /// How long the pet holds a prompt before Claude shows its own.
    pub hold_seconds: u64,
    pub sound: bool,
    pub show_quiet_sessions: bool,
    pub quiet_after_hours: f64,
    pub shortcut: String,
    pub whip_shortcut: String,
    pub whip_sound: bool,
    /// Whether "Open at login" has been set up: it starts on, and after that it's your call.
    pub login_chosen: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            pet: "pip".into(),
            size: "medium".into(),
            place: None,
            answer_from_pet: true,
            hold_seconds: 45,
            sound: false,
            show_quiet_sessions: true,
            quiet_after_hours: 12.0,
            shortcut: "Alt+Shift+P".into(),
            whip_shortcut: "Control+Shift+W".into(),
            whip_sound: true,
            login_chosen: false,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Config {
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_old_configs_and_fills_defaults() {
        let c: Config = serde_json::from_str(r#"{"pet":"builtin:mochi","size":"small","place":{"displayId":1,"fx":0.5,"fy":0.9},"position":{"x":1}}"#).unwrap();
        assert_eq!(c.pet, "builtin:mochi");
        assert_eq!(c.hold_seconds, 45);
        assert!(c.place.as_ref().unwrap().display.is_none());
        assert_eq!(c.whip_shortcut, "Control+Shift+W");
    }
}
