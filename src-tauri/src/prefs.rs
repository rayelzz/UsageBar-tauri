use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prefs {
    pub locked: bool,
    pub click_through: bool,
    pub edge: String,
    pub along: f64,
    pub floating_x: f64,
    pub floating_y: f64,
    pub refresh_interval: u64,
    pub launch_at_login: bool,
    #[serde(default)]
    pub screen_name: String,
    #[serde(default)]
    pub display_style: String,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            locked: false,
            click_through: true,
            edge: "right".into(),
            along: -1.0,
            floating_x: 0.0,
            floating_y: 0.0,
            refresh_interval: 60,
            launch_at_login: false,
            screen_name: String::new(),
            display_style: "full".into(),
        }
    }
}

fn path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".usagebar")
        .join("prefs.json")
}

pub fn load() -> Prefs {
    let path = path();
    fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(prefs: &Prefs) {
    let path = path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(prefs) {
        let _ = fs::write(path, text);
    }
}
