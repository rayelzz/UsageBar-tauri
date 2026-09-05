use crate::providers::{CreditNotice, CreditNoticeItem, ProviderSnapshot, ResetCredit, ResetNotice};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static STATE_LOCK: Mutex<()> = Mutex::new(());

fn lock_state() -> std::sync::MutexGuard<'static, ()> {
    STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

const USED_MIN: f64 = 1.0;
const ZERO_MAX: f64 = 0.5;
const ALERT_MS: i64 = 6 * 60 * 60 * 1000;
/// Treat a drop as the advertised cycle if it happens at or after the stored
/// reset time, or up to this early (poll / clock skew).
const SCHEDULED_EARLY_MS: i64 = 3 * 60 * 1000;
const HOUR_MS: i64 = 60 * 60 * 1000;
const DAY_MS: i64 = 24 * HOUR_MS;
const CREDIT_MILESTONES: [&str; 6] = ["1d", "h5", "h4", "h3", "h2", "h1"];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageState {
    #[serde(default)]
    last: HashMap<String, HashMap<String, f64>>,
    #[serde(default)]
    last_resets: HashMap<String, HashMap<String, i64>>,
    #[serde(default)]
    alerts: HashMap<String, StoredAlert>,
    #[serde(default)]
    credit_fired: HashMap<String, i64>,
    #[serde(default)]
    credit_alerts: HashMap<String, StoredCreditAlert>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredAlert {
    from_percent: f64,
    labels: Vec<String>,
    at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredCreditAlert {
    items: Vec<CreditNoticeItem>,
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
    let Ok(text) = serde_json::to_string_pretty(state) else {
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

fn reset_map(snap: &ProviderSnapshot) -> HashMap<String, i64> {
    let mut map = HashMap::new();
    for metric in &snap.metrics {
        let Some(at) = metric.resets_at.filter(|at| *at > 0) else {
            continue;
        };
        map.insert(metric.id.clone(), at);
        map.insert(format!("label:{}", metric.label), at);
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

fn prev_reset(prev_resets: &HashMap<String, i64>, id: &str, label: Option<&str>) -> Option<i64> {
    prev_resets
        .get(id)
        .copied()
        .or_else(|| label.and_then(|l| prev_resets.get(&format!("label:{l}")).copied()))
}

fn is_reset(old: f64, new_pct: f64) -> bool {
    old >= USED_MIN && new_pct <= ZERO_MAX
}

fn scheduled_reset(prev_at: Option<i64>, now: i64) -> bool {
    match prev_at {
        Some(at) if at > 0 => now + SCHEDULED_EARLY_MS >= at,
        _ => false,
    }
}

fn detect(
    prev: &HashMap<String, f64>,
    prev_resets: &HashMap<String, i64>,
    snap: &ProviderSnapshot,
    now: i64,
) -> Vec<(String, f64)> {
    let mut hits = Vec::new();
    for metric in &snap.metrics {
        let Some(old) = prev_for(prev, &metric.id, Some(&metric.label)) else {
            continue;
        };
        if !is_reset(old, metric.percent) {
            continue;
        }
        if scheduled_reset(prev_reset(prev_resets, &metric.id, Some(&metric.label)), now) {
            continue;
        }
        hits.push((metric.label.clone(), old));
    }
    if hits.is_empty() {
        if let Some(new_pct) = snap.headline_percent {
            if let Some(old) = prev.get("_headline").copied() {
                if is_reset(old, new_pct) {
                    let first = snap.metrics.first();
                    let at = first.and_then(|m| prev_reset(prev_resets, &m.id, Some(&m.label)));
                    if !scheduled_reset(at, now) {
                        hits.push((label_for(snap, "_headline"), old));
                    }
                }
            }
        }
    }
    hits
}

fn credit_identity(credit: &ResetCredit) -> String {
    if !credit.id.is_empty() {
        credit.id.clone()
    } else {
        format!("{}@{}", credit.title, credit.expires_at.unwrap_or(0))
    }
}

fn credit_fired_key(provider: &str, identity: &str, milestone: &str) -> String {
    format!("{provider}:{identity}:{milestone}")
}

fn credit_milestone(remaining: i64) -> Option<&'static str> {
    if remaining <= 0 {
        None
    } else if remaining <= HOUR_MS {
        Some("h1")
    } else if remaining <= 2 * HOUR_MS {
        Some("h2")
    } else if remaining <= 3 * HOUR_MS {
        Some("h3")
    } else if remaining <= 4 * HOUR_MS {
        Some("h4")
    } else if remaining <= 5 * HOUR_MS {
        Some("h5")
    } else if remaining <= DAY_MS {
        Some("1d")
    } else {
        None
    }
}

fn live_credit_identities(snap: &ProviderSnapshot, now: i64) -> HashSet<String> {
    snap.reset_credits
        .iter()
        .filter_map(|credit| {
            let exp = credit.expires_at?;
            if exp <= now {
                None
            } else {
                Some(credit_identity(credit))
            }
        })
        .collect()
}

fn prune_credit_for_provider(state: &mut UsageState, provider: &str, live: &HashSet<String>) {
    let prefix = format!("{provider}:");
    state.credit_fired.retain(|key, _| {
        let Some(rest) = key.strip_prefix(&prefix) else {
            return true;
        };
        live.iter().any(|identity| {
            CREDIT_MILESTONES
                .iter()
                .any(|milestone| rest == format!("{identity}:{milestone}"))
        })
    });
    if let Some(alert) = state.credit_alerts.get_mut(provider) {
        alert.items.retain(|item| live.contains(&item.id));
        if alert.items.is_empty() {
            state.credit_alerts.remove(provider);
        }
    }
}

fn detect_credit_items(
    snap: &ProviderSnapshot,
    fired: &HashMap<String, i64>,
    now: i64,
) -> Vec<CreditNoticeItem> {
    let mut items = Vec::new();
    for credit in &snap.reset_credits {
        let Some(expires_at) = credit.expires_at else {
            continue;
        };
        if expires_at <= now {
            continue;
        }
        let Some(milestone) = credit_milestone(expires_at - now) else {
            continue;
        };
        let identity = credit_identity(credit);
        let key = credit_fired_key(&snap.id, &identity, milestone);
        if fired.contains_key(&key) {
            continue;
        }
        items.push(CreditNoticeItem {
            id: identity,
            title: credit.title.clone(),
            expires_at: Some(expires_at),
            milestone: milestone.into(),
        });
    }
    items
}

fn refresh_credit_items(snap: &ProviderSnapshot, items: &mut [CreditNoticeItem]) {
    for item in items {
        if let Some(credit) = snap
            .reset_credits
            .iter()
            .find(|credit| credit_identity(credit) == item.id)
        {
            item.title = credit.title.clone();
            item.expires_at = credit.expires_at;
        }
    }
}

fn apply_credit_notice(state: &mut UsageState, snap: &mut ProviderSnapshot, now: i64) {
    let live = live_credit_identities(snap, now);
    prune_credit_for_provider(state, &snap.id, &live);
    if snap.reset_notice.is_some() {
        return;
    }
    let hits = detect_credit_items(snap, &state.credit_fired, now);
    let mut items = state
        .credit_alerts
        .get(&snap.id)
        .map(|alert| alert.items.clone())
        .unwrap_or_default();
    let mut fresh = false;
    for hit in hits {
        if let Some(existing) = items.iter_mut().find(|item| item.id == hit.id) {
            if existing.milestone != hit.milestone {
                *existing = hit;
                fresh = true;
            }
        } else {
            items.push(hit);
            fresh = true;
        }
    }
    items.retain(|item| live.contains(&item.id));
    if items.is_empty() {
        state.credit_alerts.remove(&snap.id);
        return;
    }
    refresh_credit_items(snap, &mut items);
    state.credit_alerts.insert(
        snap.id.clone(),
        StoredCreditAlert {
            items: items.clone(),
            at: now,
        },
    );
    snap.credit_notice = Some(CreditNotice { fresh, items });
}

fn restore_credit_notice(state: &UsageState, snap: &mut ProviderSnapshot) {
    if snap.reset_notice.is_some() {
        return;
    }
    if let Some(alert) = state.credit_alerts.get(&snap.id) {
        snap.credit_notice = Some(CreditNotice {
            fresh: false,
            items: alert.items.clone(),
        });
    }
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
    let _guard = lock_state();
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
            restore_credit_notice(&state, snap);
            continue;
        }
        let current = metric_map(snap);
        let current_resets = reset_map(snap);
        if let Some(prev) = state.last.get(&snap.id) {
            let prev_resets = state.last_resets.get(&snap.id).cloned().unwrap_or_default();
            let hits = detect(prev, &prev_resets, snap, now);
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
        apply_credit_notice(&mut state, snap, now);
        state.last.insert(snap.id.clone(), current);
        state.last_resets.insert(snap.id.clone(), current_resets);
    }
    save(&state);
}

pub fn dismiss(id: &str) {
    let _guard = lock_state();
    let mut state = load();
    state.alerts.remove(id);
    save(&state);
}

pub fn dismiss_credit(id: &str) {
    let _guard = lock_state();
    let mut state = load();
    dismiss_credit_in(&mut state, id, now_ms());
    save(&state);
}

fn dismiss_credit_in(state: &mut UsageState, id: &str, now: i64) {
    let Some(alert) = state.credit_alerts.remove(id) else {
        return;
    };
    for item in alert.items {
        state
            .credit_fired
            .insert(credit_fired_key(id, &item.id, &item.milestone), now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{ProviderSnapshot, ResetCredit, ResetNotice, UsageMetric};

    fn snap(id: &str, metrics: Vec<(&str, &str, f64)>) -> ProviderSnapshot {
        snap_resets(
            id,
            metrics
                .into_iter()
                .map(|(mid, label, percent)| (mid, label, percent, None))
                .collect(),
        )
    }

    fn snap_resets(id: &str, metrics: Vec<(&str, &str, f64, Option<i64>)>) -> ProviderSnapshot {
        let list: Vec<UsageMetric> = metrics
            .into_iter()
            .map(|(mid, label, percent, resets_at)| UsageMetric {
                id: mid.into(),
                label: label.into(),
                percent,
                resets_at,
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
            credit_notice: None,
            reset_credits: vec![],
        }
    }

    fn credit(id: &str, title: &str, expires_at: Option<i64>) -> ResetCredit {
        ResetCredit {
            id: id.into(),
            title: title.into(),
            status: "available".into(),
            granted_at: None,
            expires_at,
        }
    }

    fn snap_credits(id: &str, credits: Vec<ResetCredit>) -> ProviderSnapshot {
        let mut snap = snap(id, vec![("weekly", "Weekly limit", 10.0)]);
        snap.reset_credits = credits;
        snap
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
        let hits = detect(&prev, &HashMap::new(), &current, 0);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, "5-hour window");
        assert_eq!(hits[0].1, 74.0);
    }

    #[test]
    fn ignores_already_zero_and_missing_history() {
        let prev = HashMap::from([("five_hour".into(), 0.0)]);
        let current = snap("codex", vec![("five_hour", "5-hour window", 0.0)]);
        assert!(detect(&prev, &HashMap::new(), &current, 0).is_empty());
        assert!(detect(&HashMap::new(), &HashMap::new(), &current, 0).is_empty());
    }

    #[test]
    fn ignores_drop_at_stored_reset_time() {
        let reset_at = 1_800_000_000_000;
        let prev = HashMap::from([("five_hour".into(), 74.0)]);
        let prev_resets = HashMap::from([("five_hour".into(), reset_at)]);
        let current = snap_resets(
            "codex",
            vec![("five_hour", "5-hour window", 0.0, Some(reset_at + 5 * 60 * 60 * 1000))],
        );
        assert!(detect(&prev, &prev_resets, &current, reset_at).is_empty());
        assert!(detect(&prev, &prev_resets, &current, reset_at + 60_000).is_empty());
    }

    #[test]
    fn detects_drop_before_stored_reset_time() {
        let reset_at = 1_800_000_000_000;
        let prev = HashMap::from([("five_hour".into(), 74.0)]);
        let prev_resets = HashMap::from([("five_hour".into(), reset_at)]);
        let current = snap("codex", vec![("five_hour", "5-hour window", 0.0)]);
        let hits = detect(&prev, &prev_resets, &current, reset_at - 10 * 60 * 1000);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, "5-hour window");
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
        let hits = detect(&prev, &HashMap::new(), &current, 0);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, "Weekly limit");
        assert_eq!(hits[0].1, 74.0);
    }

    #[test]
    fn detects_headline_drop_for_any_provider() {
        let prev = HashMap::from([("_headline".into(), 61.0)]);
        let current = snap("cursor", vec![("included", "Included usage", 0.0)]);
        let hits = detect(&prev, &HashMap::new(), &current, 0);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].1, 61.0);
    }

    #[test]
    fn ignores_headline_drop_when_first_metric_was_scheduled() {
        let reset_at = 1_800_000_000_000;
        let prev = HashMap::from([("_headline".into(), 61.0), ("included".into(), 61.0)]);
        let prev_resets = HashMap::from([("included".into(), reset_at)]);
        let current = snap("cursor", vec![("included", "Included usage", 0.0)]);
        assert!(detect(&prev, &prev_resets, &current, reset_at + 1).is_empty());
    }

    #[test]
    fn credit_milestone_picks_finest_bucket() {
        assert_eq!(credit_milestone(26 * HOUR_MS), None);
        assert_eq!(credit_milestone(20 * HOUR_MS), Some("1d"));
        assert_eq!(credit_milestone(5 * HOUR_MS), Some("h5"));
        assert_eq!(credit_milestone(4 * HOUR_MS + 1), Some("h5"));
        assert_eq!(credit_milestone(4 * HOUR_MS), Some("h4"));
        assert_eq!(credit_milestone((3.5 * HOUR_MS as f64) as i64), Some("h4"));
        assert_eq!(credit_milestone((2.5 * HOUR_MS as f64) as i64), Some("h3"));
        assert_eq!(credit_milestone(2 * HOUR_MS), Some("h2"));
        assert_eq!(credit_milestone(HOUR_MS), Some("h1"));
        assert_eq!(credit_milestone(1), Some("h1"));
        assert_eq!(credit_milestone(0), None);
        assert_eq!(credit_milestone(-1), None);
    }

    #[test]
    fn credit_identity_falls_back_when_id_empty() {
        assert_eq!(
            credit_identity(&credit("", "Full reset", Some(99))),
            "Full reset@99"
        );
        assert_eq!(credit_identity(&credit("tok-1", "Full reset", Some(99))), "tok-1");
    }

    #[test]
    fn credit_detect_skips_missing_or_expired() {
        let now = 1_800_000_000_000;
        let snap = snap_credits(
            "codex",
            vec![
                credit("a", "Full reset", None),
                credit("b", "Full reset", Some(now)),
                credit("c", "Full reset", Some(now - 1)),
                credit("d", "Full reset", Some(now + 30 * HOUR_MS)),
            ],
        );
        assert!(detect_credit_items(&snap, &HashMap::new(), now).is_empty());
    }

    #[test]
    fn credit_detect_only_finest_unfired_bucket() {
        let now = 1_800_000_000_000;
        let snap = snap_credits(
            "codex",
            vec![credit("card", "Full reset", Some(now + (2.5 * HOUR_MS as f64) as i64))],
        );
        let hits = detect_credit_items(&snap, &HashMap::new(), now);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].milestone, "h3");
        assert_eq!(hits[0].id, "card");
    }

    #[test]
    fn credit_detect_skips_already_fired_milestone() {
        let now = 1_800_000_000_000;
        let snap = snap_credits(
            "codex",
            vec![credit("card", "Full reset", Some(now + 20 * HOUR_MS))],
        );
        let fired = HashMap::from([("codex:card:1d".into(), now)]);
        assert!(detect_credit_items(&snap, &fired, now).is_empty());
    }

    #[test]
    fn credit_notice_skips_when_reset_notice_present() {
        let now = 1_800_000_000_000;
        let mut snap = snap_credits(
            "codex",
            vec![credit("card", "Full reset", Some(now + 20 * HOUR_MS))],
        );
        snap.reset_notice = Some(ResetNotice {
            fresh: true,
            from_percent: 80.0,
            labels: vec!["Weekly limit".into()],
        });
        let mut state = UsageState::default();
        apply_credit_notice(&mut state, &mut snap, now);
        assert!(snap.credit_notice.is_none());
        assert!(state.credit_alerts.is_empty());
    }

    #[test]
    fn credit_notice_keeps_pending_and_marks_fired_on_dismiss() {
        let now = 1_800_000_000_000;
        let mut snap = snap_credits(
            "codex",
            vec![credit("", "Full reset", Some(now + 20 * HOUR_MS))],
        );
        let mut state = UsageState::default();
        apply_credit_notice(&mut state, &mut snap, now);
        let notice = snap.credit_notice.expect("credit notice");
        assert!(notice.fresh);
        assert_eq!(notice.items.len(), 1);
        assert_eq!(notice.items[0].id, "Full reset@1800072000000");
        assert_eq!(notice.items[0].milestone, "1d");

        snap.credit_notice = None;
        apply_credit_notice(&mut state, &mut snap, now + 60_000);
        let again = snap.credit_notice.expect("pending credit notice");
        assert!(!again.fresh);
        assert_eq!(again.items[0].milestone, "1d");

        dismiss_credit_in(&mut state, "codex", now + 120_000);
        snap.credit_notice = None;
        apply_credit_notice(&mut state, &mut snap, now + 180_000);
        assert!(snap.credit_notice.is_none());
    }

    #[test]
    fn credit_notice_lists_multiple_unfired_cards() {
        let now = 1_800_000_000_000;
        let mut snap = snap_credits(
            "glm",
            vec![
                credit("five", "5-hour reset", Some(now + HOUR_MS)),
                credit("week", "Weekly reset", Some(now + 20 * HOUR_MS)),
            ],
        );
        let mut state = UsageState::default();
        apply_credit_notice(&mut state, &mut snap, now);
        let notice = snap.credit_notice.expect("credit notice");
        assert_eq!(notice.items.len(), 2);
        assert_eq!(notice.items[0].milestone, "h1");
        assert_eq!(notice.items[1].milestone, "1d");
    }

    #[test]
    fn pending_credit_replaces_same_card_milestone() {
        let now = 1_800_000_000_000;
        let mut snap = snap_credits(
            "codex",
            vec![credit("full", "Full reset", Some(now + DAY_MS))],
        );
        let mut state = UsageState::default();
        apply_credit_notice(&mut state, &mut snap, now);
        assert_eq!(snap.credit_notice.as_ref().unwrap().items[0].milestone, "1d");
        apply_credit_notice(&mut state, &mut snap, now + DAY_MS - HOUR_MS);
        let notice = snap.credit_notice.expect("still pending");
        assert_eq!(notice.items.len(), 1);
        assert_eq!(notice.items[0].milestone, "h1");
        assert!(notice.fresh);
    }
}
