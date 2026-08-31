use crate::providers::{ProviderSnapshot, ResetNotice};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const USED_MIN: f64 = 1.0;
const ZERO_MAX: f64 = 0.5;
const ALERT_MS: i64 = 6 * 60 * 60 * 1000;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageState {
    #[serde(default)]
    last: HashMap<String, HashMap<String, f64>>,
    #[serde(default)]
    alerts: HashMap<String, StoredAlert>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredAlert {
    from_percent: f64,
    labels: Vec<String>,
    at: i64,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".usagebar")
        .join("usage-state.json")
}

fn load() -> UsageState {
    fs::read_to_string(path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save(state: &UsageState) {
    let path = path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(state) {
        let _ = fs::write(path, text);
    }
}

fn metric_map(snap: &ProviderSnapshot) -> HashMap<String, f64> {
    let mut map = HashMap::new();
    for metric in &snap.metrics {
        map.insert(metric.id.clone(), metric.percent);
        map.insert(format!("label:{}", metric.label), metric.percent);
    }
    if let Some(pct) = snap.headline_percent {
        map.insert("_headline".into(), pct);
    }
    map
}

fn label_for(snap: &ProviderSnapshot, id: &str) -> String {
    if id == "_headline" {
        return snap
            .metrics
            .first()
            .map(|m| m.label.clone())
            .unwrap_or_else(|| "Usage".into());
    }
    snap.metrics
        .iter()
        .find(|m| m.id == id)
        .map(|m| m.label.clone())
        .unwrap_or_else(|| "Usage".into())
}

fn usable(snap: &ProviderSnapshot) -> bool {
    snap.error.is_none()
        && (snap.headline_percent.is_some() || !snap.metrics.is_empty())
}

fn prev_for(prev: &HashMap<String, f64>, id: &str, label: Option<&str>) -> Option<f64> {
    prev.get(id)
        .copied()
        .or_else(|| label.and_then(|l| prev.get(&format!("label:{l}")).copied()))
}

fn is_reset(old: f64, new_pct: f64) -> bool {
    old >= USED_MIN && new_pct <= ZERO_MAX
}

fn detect(prev: &HashMap<String, f64>, snap: &ProviderSnapshot) -> Vec<(String, f64)> {
    let mut hits = Vec::new();
    for metric in &snap.metrics {
        let Some(old) = prev_for(prev, &metric.id, Some(&metric.label)) else {
            continue;
        };
        if is_reset(old, metric.percent) {
            hits.push((metric.label.clone(), old));
        }
    }
    if hits.is_empty() {
        if let Some(new_pct) = snap.headline_percent {
            if let Some(old) = prev.get("_headline").copied() {
                if is_reset(old, new_pct) {
                    hits.push((label_for(snap, "_headline"), old));
                }
            }
        }
    }
    hits
}

fn still_zero(snap: &ProviderSnapshot, alert: &StoredAlert) -> bool {
    if snap.headline_percent.unwrap_or(100.0) <= ZERO_MAX {
        return true;
    }
    if snap.metrics.is_empty() {
        return false;
    }
    alert.labels.iter().any(|label| {
        snap.metrics
            .iter()
            .any(|m| m.label == *label && m.percent <= ZERO_MAX)
    })
}

pub fn apply(snaps: &mut [ProviderSnapshot]) {
    let mut state = load();
    let now = now_ms();
    state.alerts.retain(|_, alert| now - alert.at <= ALERT_MS);
    for snap in snaps.iter_mut() {
        if !usable(snap) {
            if let Some(alert) = state.alerts.get(&snap.id) {
                snap.reset_notice = Some(ResetNotice {
                    fresh: false,
                    from_percent: alert.from_percent,
                    labels: alert.labels.clone(),
                });
            }
            continue;
        }
        let current = metric_map(snap);
        if let Some(prev) = state.last.get(&snap.id) {
            let hits = detect(prev, snap);
            if !hits.is_empty() {
                let from_percent = hits
                    .iter()
                    .map(|(_, p)| *p)
                    .fold(0.0_f64, f64::max);
                let labels: Vec<String> = hits.into_iter().map(|(l, _)| l).collect();
                state.alerts.insert(
                    snap.id.clone(),
                    StoredAlert {
                        from_percent,
                        labels: labels.clone(),
                        at: now,
                    },
                );
                snap.reset_notice = Some(ResetNotice {
                    fresh: true,
                    from_percent,
                    labels,
                });
            }
        }
        if snap.reset_notice.is_none() {
            if let Some(alert) = state.alerts.get(&snap.id).cloned() {
                if still_zero(snap, &alert) {
                    snap.reset_notice = Some(ResetNotice {
                        fresh: false,
                        from_percent: alert.from_percent,
                        labels: alert.labels,
                    });
                } else {
                    state.alerts.remove(&snap.id);
                }
            }
        }
        state.last.insert(snap.id.clone(), current);
    }
    save(&state);
}

pub fn dismiss(id: &str) {
    let mut state = load();
    state.alerts.remove(id);
    save(&state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{ProviderSnapshot, UsageMetric};

    fn snap(id: &str, metrics: Vec<(&str, &str, f64)>) -> ProviderSnapshot {
        let list: Vec<UsageMetric> = metrics
            .into_iter()
            .map(|(mid, label, percent)| UsageMetric {
                id: mid.into(),
                label: label.into(),
                percent,
                resets_at: None,
            })
            .collect();
        let headline = list.first().map(|m| m.percent);
        ProviderSnapshot {
            id: id.into(),
            title: "Test".into(),
            headline_percent: headline,
            metrics: list,
            error: None,
            updated_at: 1,
            reset_notice: None,
        }
    }

    #[test]
    fn detects_metric_drop_to_zero() {
        let prev = HashMap::from([
            ("five_hour".into(), 74.0),
            ("weekly".into(), 10.0),
            ("_headline".into(), 74.0),
        ]);
        let current = snap(
            "codex",
            vec![
                ("five_hour", "5-hour window", 0.0),
                ("weekly", "Weekly limit", 10.0),
            ],
        );
        let hits = detect(&prev, &current);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, "5-hour window");
        assert_eq!(hits[0].1, 74.0);
    }

    #[test]
    fn ignores_already_zero_and_missing_history() {
        let prev = HashMap::from([("five_hour".into(), 0.0)]);
        let current = snap("codex", vec![("five_hour", "5-hour window", 0.0)]);
        assert!(detect(&prev, &current).is_empty());
        assert!(detect(&HashMap::new(), &current).is_empty());
    }

    #[test]
    fn detects_same_label_when_metric_id_changes() {
        let prev = HashMap::from([
            ("TOKENS_LIMIT-6-7".into(), 74.0),
            ("label:Weekly limit".into(), 74.0),
        ]);
        let current = snap(
            "glm",
            vec![("TOKENS_LIMIT-6-1", "Weekly limit", 0.0)],
        );
        let hits = detect(&prev, &current);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, "Weekly limit");
        assert_eq!(hits[0].1, 74.0);
    }

    #[test]
    fn detects_headline_drop_for_any_provider() {
        let prev = HashMap::from([("_headline".into(), 61.0)]);
        let current = snap("cursor", vec![("included", "Included usage", 0.0)]);
        let hits = detect(&prev, &current);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].1, 61.0);
    }
}
