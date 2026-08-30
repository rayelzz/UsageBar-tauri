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
    #[serde(default = "default_locale")]
    pub locale: String,
    #[serde(default = "default_visible_providers")]
    pub visible_providers: Vec<String>,
}

pub const CATALOG: &[&str] = &[
    "codex",
    "cursor",
    "grok",
    "glm",
    "claude",
    "copilot",
    "gemini",
    "antigravity",
];
pub const SLOT_COUNT: usize = 4;

fn default_locale() -> String {
    "en".into()
}

pub fn default_visible_providers() -> Vec<String> {
    vec![
        "codex".into(),
        "cursor".into(),
        "grok".into(),
        "glm".into(),
    ]
}

pub fn catalog_has(id: &str) -> bool {
    CATALOG.contains(&id)
}

pub fn normalize_visible(ids: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for id in ids {
        if catalog_has(id) && !out.iter().any(|x| x == id) {
            out.push(id.clone());
        }
        if out.len() == SLOT_COUNT {
            break;
        }
    }
    if out.is_empty() {
        default_visible_providers()
    } else {
        out
    }
}

pub fn display_slots(prefs: &Prefs) -> [Option<String>; SLOT_COUNT] {
    let vis = normalize_visible(&prefs.visible_providers);
    let mut slots = [None, None, None, None];
    for (i, id) in vis.into_iter().take(SLOT_COUNT).enumerate() {
        slots[i] = Some(id);
    }
    slots
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
            locale: default_locale(),
            visible_providers: default_visible_providers(),
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
    let mut prefs: Prefs = fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    prefs.visible_providers = normalize_visible(&prefs.visible_providers);
    prefs
}

pub fn save(prefs: &Prefs) {
    let path = path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let mut copy = prefs.clone();
    copy.visible_providers = normalize_visible(&copy.visible_providers);
    if let Ok(text) = serde_json::to_string_pretty(&copy) {
        let _ = fs::write(path, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_caps_dedupes_and_drops_unknown() {
        let ids = vec![
            "codex".into(),
            "codex".into(),
            "claude".into(),
            "nope".into(),
            "gemini".into(),
            "copilot".into(),
            "glm".into(),
        ];
        assert_eq!(
            normalize_visible(&ids),
            vec!["codex", "claude", "gemini", "copilot"]
        );
    }

    #[test]
    fn normalize_empty_uses_defaults() {
        assert_eq!(normalize_visible(&[]), default_visible_providers());
    }

    #[test]
    fn display_slots_pad_to_four() {
        let mut prefs = Prefs::default();
        prefs.visible_providers = vec!["claude".into()];
        assert_eq!(
            display_slots(&prefs),
            [Some("claude".into()), None, None, None]
        );
    }
}
