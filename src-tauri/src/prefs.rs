use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Prefs {
    pub locked: bool,
    pub click_through: bool,
    pub edge: String,
    pub along: f64,
    pub floating_x: f64,
    pub floating_y: f64,
    pub refresh_interval: u64,
    pub launch_at_login: bool,
    pub screen_name: String,
    pub display_style: String,
    pub display_value: String,
    pub locale: String,
    pub visible_providers: Vec<String>,
    pub auto_check_update: bool,
    pub skipped_update_version: String,
    /// Last placed origin in logical pixels. Negative means unset.
    pub last_x: f64,
    pub last_y: f64,
}

pub const CATALOG: &[&str] = &[
    "codex",
    "cursor",
    "grok",
    "glm",
    "zcode",
    "claude",
    "copilot",
    "gemini",
    "antigravity",
];
pub const SLOT_MIN: usize = 1;
pub const SLOT_MAX: usize = 10;
const UNSET: f64 = -1.0;

pub fn default_locale() -> String {
    "en".into()
}

pub fn default_display_value() -> String {
    "used".into()
}

pub fn normalize_display_value(value: &str) -> String {
    if value == "remaining" {
        "remaining".into()
    } else {
        "used".into()
    }
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
        if out.len() == SLOT_MAX {
            break;
        }
    }
    if out.is_empty() {
        default_visible_providers()
    } else {
        out
    }
}

pub fn display_slots(prefs: &Prefs) -> Vec<String> {
    normalize_visible(&prefs.visible_providers)
}

pub fn slot_count(prefs: &Prefs) -> usize {
    display_slots(prefs).len().clamp(SLOT_MIN, SLOT_MAX)
}

pub fn is_unset(v: f64) -> bool {
    !v.is_finite() || v < 0.0
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            locked: false,
            click_through: true,
            edge: "right".into(),
            along: UNSET,
            floating_x: 0.0,
            floating_y: 0.0,
            refresh_interval: 60,
            launch_at_login: false,
            screen_name: String::new(),
            display_style: "full".into(),
            display_value: default_display_value(),
            locale: default_locale(),
            visible_providers: default_visible_providers(),
            auto_check_update: false,
            skipped_update_version: String::new(),
            last_x: UNSET,
            last_y: UNSET,
        }
    }
}

impl Prefs {
    pub fn sanitize(mut self) -> Self {
        if self.edge != "left"
            && self.edge != "right"
            && self.edge != "top"
            && self.edge != "bottom"
            && self.edge != "floating"
        {
            self.edge = "right".into();
        }
        if self.display_style != "icons" {
            self.display_style = "full".into();
        }
        self.display_value = normalize_display_value(&self.display_value);
        self.locale = if self.locale == "zh" { "zh".into() } else { "en".into() };
        self.visible_providers = normalize_visible(&self.visible_providers);
        if !self.along.is_finite() {
            self.along = UNSET;
        }
        if !self.last_x.is_finite() {
            self.last_x = UNSET;
        }
        if !self.last_y.is_finite() {
            self.last_y = UNSET;
        }
        if self.refresh_interval != 0
            && ![15, 30, 60, 120, 300, 600].contains(&self.refresh_interval)
        {
            self.refresh_interval = 60;
        }
        self
    }
}

/// Keep already-saved values when the incoming payload omitted them or sent sentinels.
pub fn merge(base: Prefs, mut next: Prefs) -> Prefs {
    if next.visible_providers.is_empty() {
        next.visible_providers = base.visible_providers;
    }
    if is_unset(next.along) && !is_unset(base.along) {
        next.along = base.along;
    }
    if next.screen_name.is_empty() && !base.screen_name.is_empty() {
        next.screen_name = base.screen_name;
    }
    if is_unset(next.last_x) && !is_unset(base.last_x) {
        next.last_x = base.last_x;
    }
    if is_unset(next.last_y) && !is_unset(base.last_y) {
        next.last_y = base.last_y;
    }
    next.sanitize()
}

/// Full-window snapshots are often stale. Keep geometry unless the edge changed
/// (snap), and never take `visible_providers` from `set_prefs` — that list is
/// owned by `set_visible_providers` so the bar cannot overwrite a settings edit.
pub fn apply_incoming(base: Prefs, incoming: Prefs) -> Prefs {
    let edge_changed = incoming.edge != base.edge;
    let mut prefs = merge(base.clone(), incoming);
    prefs.visible_providers = base.visible_providers;
    if !edge_changed {
        prefs.along = base.along;
        prefs.last_x = base.last_x;
        prefs.last_y = base.last_y;
        prefs.floating_x = base.floating_x;
        prefs.floating_y = base.floating_y;
        prefs.screen_name = base.screen_name;
    }
    prefs
}

pub fn tray_needs_update(before: &Prefs, after: &Prefs) -> bool {
    before.locale != after.locale
        || before.refresh_interval != after.refresh_interval
        || before.locked != after.locked
        || before.click_through != after.click_through
        || before.edge != after.edge
        || before.display_style != after.display_style
        || before.display_value != after.display_value
        || before.launch_at_login != after.launch_at_login
}

pub fn parse_text(text: &str) -> Prefs {
    if let Ok(prefs) = serde_json::from_str::<Prefs>(text) {
        return prefs.sanitize();
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
        if let Some(obj) = value.as_object() {
            let mut prefs = Prefs::default();
            if let Ok(partial) = serde_json::from_value::<Prefs>(serde_json::Value::Object(obj.clone()))
            {
                prefs = partial;
            }
            return prefs.sanitize();
        }
    }
    Prefs::default()
}

fn path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".usagebar")
        .join("prefs.json")
}

pub fn load() -> Prefs {
    let path = path();
    match fs::read_to_string(&path) {
        Ok(text) => {
            if serde_json::from_str::<Prefs>(&text).is_err() {
                let bak = path.with_extension("json.bak");
                let _ = fs::copy(&path, bak);
            }
            parse_text(&text)
        }
        Err(_) => Prefs::default(),
    }
}

pub fn save(prefs: &Prefs) {
    let path = path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let copy = prefs.clone().sanitize();
    let Ok(text) = serde_json::to_string_pretty(&copy) else {
        return;
    };
    let tmp = path.with_extension("json.tmp");
    if fs::write(&tmp, &text).is_err() {
        let _ = fs::write(&path, text);
        return;
    }
    if fs::rename(&tmp, &path).is_err() {
        let _ = fs::remove_file(&path);
        if fs::rename(&tmp, &path).is_err() {
            let _ = fs::write(&path, text);
            let _ = fs::remove_file(&tmp);
        }
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
            vec!["codex", "claude", "gemini", "copilot", "glm"]
        );
    }

    #[test]
    fn normalize_empty_uses_defaults() {
        assert_eq!(normalize_visible(&[]), default_visible_providers());
    }

    #[test]
    fn display_slots_follow_selection() {
        let mut prefs = Prefs::default();
        prefs.visible_providers = vec!["claude".into()];
        assert_eq!(display_slots(&prefs), vec!["claude".to_string()]);
        assert_eq!(slot_count(&prefs), 1);
    }

    #[test]
    fn normalize_caps_at_ten() {
        let ids: Vec<String> = CATALOG.iter().map(|s| (*s).to_string()).cycle().take(20).collect();
        assert_eq!(normalize_visible(&ids).len(), CATALOG.len().min(SLOT_MAX));
    }

    #[test]
    fn auto_check_update_defaults_off() {
        let prefs = Prefs::default();
        assert!(!prefs.auto_check_update);
        assert!(prefs.skipped_update_version.is_empty());
        let parsed = parse_text(
            r#"{"locked":false,"clickThrough":true,"edge":"right","along":-1.0,"floatingX":0.0,"floatingY":0.0,"refreshInterval":60,"launchAtLogin":false}"#,
        );
        assert!(!parsed.auto_check_update);
        assert!(parsed.skipped_update_version.is_empty());
        assert_eq!(parsed.display_value, "used");
    }

    #[test]
    fn display_value_defaults_to_used() {
        let prefs = Prefs::default();
        assert_eq!(prefs.display_value, "used");
        assert_eq!(normalize_display_value("remaining"), "remaining");
        assert_eq!(normalize_display_value("used"), "used");
        assert_eq!(normalize_display_value("nope"), "used");
    }

    #[test]
    fn update_keeps_custom_providers_and_order() {
        let parsed = parse_text(
            r#"{
                "locked":false,
                "clickThrough":true,
                "edge":"right",
                "along":431.0,
                "floatingX":0.0,
                "floatingY":0.0,
                "refreshInterval":60,
                "launchAtLogin":false,
                "screenName":"Monitor #8193",
                "locale":"zh",
                "visibleProviders":["codex","grok","cursor","glm"]
            }"#,
        );
        assert_eq!(
            parsed.visible_providers,
            vec!["codex", "grok", "cursor", "glm"]
        );
        assert_eq!(parsed.along, 431.0);
        assert_eq!(parsed.edge, "right");
        assert_eq!(parsed.locale, "zh");
        assert_eq!(parsed.display_value, "used");
    }

    #[test]
    fn partial_json_does_not_reset_known_keys() {
        let parsed = parse_text(r#"{"edge":"left","along":120.5,"visibleProviders":["claude","gemini"]}"#);
        assert_eq!(parsed.edge, "left");
        assert_eq!(parsed.along, 120.5);
        assert_eq!(parsed.visible_providers, vec!["claude", "gemini"]);
        assert!(parsed.click_through);
    }

    #[test]
    fn corrupt_json_falls_back_without_panic() {
        let parsed = parse_text("{not json");
        assert_eq!(parsed.visible_providers, default_visible_providers());
        assert_eq!(parsed.edge, "right");
    }

    #[test]
    fn merge_keeps_saved_position_and_providers() {
        let mut base = Prefs::default();
        base.along = 400.0;
        base.screen_name = "Built-in".into();
        base.last_x = 1800.0;
        base.last_y = 400.0;
        base.visible_providers = vec!["claude".into(), "codex".into()];
        let mut incoming = Prefs::default();
        incoming.visible_providers.clear();
        let merged = merge(base, incoming);
        assert_eq!(merged.along, 400.0);
        assert_eq!(merged.screen_name, "Built-in");
        assert_eq!(merged.last_x, 1800.0);
        assert_eq!(merged.last_y, 400.0);
        assert_eq!(merged.visible_providers, vec!["claude", "codex"]);
    }

    #[test]
    fn merge_accepts_new_provider_order() {
        let base = Prefs::default();
        let mut incoming = Prefs::default();
        incoming.visible_providers = vec!["glm".into(), "zcode".into()];
        incoming.along = 10.0;
        let merged = merge(base, incoming);
        assert_eq!(merged.visible_providers, vec!["glm", "zcode"]);
        assert_eq!(merged.along, 10.0);
    }

    #[test]
    fn apply_incoming_keeps_geometry_for_settings_only_changes() {
        let mut base = Prefs::default();
        base.along = 431.0;
        base.last_x = 1875.0;
        base.last_y = 431.0;
        base.screen_name = "Built-in".into();
        base.refresh_interval = 60;
        let mut incoming = base.clone();
        incoming.refresh_interval = 15;
        incoming.last_x = 100.0;
        incoming.last_y = 20.0;
        incoming.along = 20.0;
        incoming.screen_name = "stale".into();
        let applied = apply_incoming(base, incoming);
        assert_eq!(applied.refresh_interval, 15);
        assert_eq!(applied.last_x, 1875.0);
        assert_eq!(applied.last_y, 431.0);
        assert_eq!(applied.along, 431.0);
        assert_eq!(applied.screen_name, "Built-in");
    }

    #[test]
    fn apply_incoming_ignores_stale_provider_list() {
        let mut base = Prefs::default();
        base.visible_providers = vec!["glm".into(), "zcode".into()];
        let mut incoming = base.clone();
        incoming.visible_providers = vec!["codex".into()];
        incoming.display_value = "remaining".into();
        let applied = apply_incoming(base, incoming);
        assert_eq!(applied.visible_providers, vec!["glm", "zcode"]);
        assert_eq!(applied.display_value, "remaining");
    }

    #[test]
    fn apply_incoming_keeps_geometry_when_style_changes() {
        let mut base = Prefs::default();
        base.display_style = "full".into();
        base.last_x = 1875.0;
        base.last_y = 400.0;
        base.along = 400.0;
        let mut incoming = base.clone();
        incoming.display_style = "icons".into();
        incoming.last_x = 10.0;
        incoming.last_y = 10.0;
        let applied = apply_incoming(base, incoming);
        assert_eq!(applied.display_style, "icons");
        assert_eq!(applied.last_x, 1875.0);
        assert_eq!(applied.last_y, 400.0);
        assert_eq!(applied.along, 400.0);
    }

    #[test]
    fn apply_incoming_takes_geometry_when_edge_changes() {
        let mut base = Prefs::default();
        base.edge = "right".into();
        base.along = 400.0;
        let mut incoming = base.clone();
        incoming.edge = "left".into();
        incoming.along = 120.0;
        let applied = apply_incoming(base, incoming);
        assert_eq!(applied.edge, "left");
        assert_eq!(applied.along, 120.0);
    }

    #[test]
    fn tray_needs_update_ignores_geometry() {
        let mut before = Prefs::default();
        before.last_x = 10.0;
        let mut after = before.clone();
        after.last_x = 99.0;
        after.along = 50.0;
        assert!(!tray_needs_update(&before, &after));
        after.locale = "zh".into();
        assert!(tray_needs_update(&before, &after));
    }
}
