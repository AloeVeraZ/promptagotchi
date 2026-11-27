//! Where everything lives on disk.
use std::path::PathBuf;
use std::sync::OnceLock;

pub struct Paths {
    pub home: PathBuf,
    /// ~/.promptagotchi (or $PROMPTAGOTCHI_HOME)
    pub data_dir: PathBuf,
    /// Pets that ship with the app.
    pub builtin_pets: PathBuf,
    /// The hook script that ships with the app (copied into data_dir at launch).
    pub hook_src: PathBuf,
    /// docs/ that ship with the app.
    pub docs: PathBuf,
}

static PATHS: OnceLock<Paths> = OnceLock::new();

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// Call once at startup with the app's resource folder (the bundle's Resources, or the repo).
pub fn init(resources: PathBuf) {
    let _ = PATHS.set(build(resources));
}

fn build(resources: PathBuf) -> Paths {
    let home = home();
    let data_dir = std::env::var_os("PROMPTAGOTCHI_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".promptagotchi"));
    Paths {
        builtin_pets: resources.join("pets"),
        hook_src: resources.join("hooks").join("promptagotchi-hook.sh"),
        docs: resources.join("docs"),
        home,
        data_dir,
    }
}

pub fn get() -> &'static Paths {
    PATHS.get_or_init(|| build(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")))
}

impl Paths {
    pub fn config_file(&self) -> PathBuf { self.data_dir.join("config.json") }
    pub fn token_file(&self) -> PathBuf { self.data_dir.join("token") }
    pub fn port_file(&self) -> PathBuf { self.data_dir.join("port") }
    pub fn hook_script(&self) -> PathBuf { self.data_dir.join("promptagotchi-hook.sh") }
    pub fn user_pets(&self) -> PathBuf { self.data_dir.join("pets") }
    pub fn codex_pets(&self) -> PathBuf {
        std::env::var_os("CODEX_HOME").map(PathBuf::from).unwrap_or_else(|| self.home.join(".codex")).join("pets")
    }
    pub fn petdex_pets(&self) -> PathBuf { self.home.join(".petdex").join("pets") }
    pub fn claude_dir(&self) -> PathBuf {
        std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|| self.home.join(".claude"))
    }
    pub fn claude_settings(&self) -> PathBuf { self.claude_dir().join("settings.json") }
    pub fn claude_sessions(&self) -> PathBuf { self.claude_dir().join("sessions") }
    pub fn claude_projects(&self) -> PathBuf { self.claude_dir().join("projects") }
    /// The command written into Claude's settings.json for the hook script.
    pub fn hook_script_ref(&self) -> String {
        if self.hook_script() == self.home.join(".promptagotchi").join("promptagotchi-hook.sh") {
            "\"$HOME/.promptagotchi/promptagotchi-hook.sh\"".to_string()
        } else {
            format!("{:?}", self.hook_script().display().to_string())
        }
    }
}
