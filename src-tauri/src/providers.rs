use chrono::{DateTime, Datelike, FixedOffset, Local, TimeZone, Utc};
use regex::Regex;
use rusqlite::Connection;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageMetric {
    pub id: String,
    pub label: String,
    pub percent: f64,
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetNotice {
    pub fresh: bool,
    pub from_percent: f64,
    pub labels: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSnapshot {
    pub id: String,
    pub title: String,
    pub headline_percent: Option<f64>,
    pub metrics: Vec<UsageMetric>,
    pub error: Option<String>,
    pub updated_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reset_notice: Option<ResetNotice>,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

fn env_or_shell(key: &str) -> Option<String> {
    if let Ok(v) = std::env::var(key) {
        if !v.is_empty() {
            return Some(v);
        }
    }
    shell_export(key)
}

fn shell_export(key: &str) -> Option<String> {
    let files = [".zshrc", ".zprofile", ".zshenv", ".bashrc", ".bash_profile", ".profile"];
    let re = Regex::new(&format!(
        r#"export\s+{}\s*=\s*['"]?([^'"\s]+)"#,
        regex::escape(key)
    ))
    .ok()?;
    for name in files {
        let text = fs::read_to_string(home().join(name)).ok()?;
        if let Some(cap) = re.captures(&text) {
            let v = cap[1].trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn json_f64(v: Option<&Value>) -> Option<f64> {
    let v = v?;
    if let Some(n) = v.as_f64() {
        return Some(n);
    }
    if let Some(n) = v.as_i64() {
        return Some(n as f64);
    }
    if let Some(s) = v.as_str() {
        return s.parse().ok();
    }
    None
}

fn beijing() -> FixedOffset {
    FixedOffset::east_opt(8 * 3600).expect("UTC+8")
}

#[derive(Clone, Copy)]
enum NaiveClock {
    Utc,
    Beijing,
}

fn parse_date(v: &Value) -> Option<i64> {
    parse_date_clock(v, NaiveClock::Utc)
}

fn parse_date_beijing(v: &Value) -> Option<i64> {
    parse_date_clock(v, NaiveClock::Beijing)
}

fn stamp_naive(ndt: chrono::NaiveDateTime, clock: NaiveClock) -> i64 {
    match clock {
        NaiveClock::Utc => Utc.from_utc_datetime(&ndt).timestamp_millis(),
        NaiveClock::Beijing => beijing()
            .from_local_datetime(&ndt)
            .single()
            .map(|dt| dt.timestamp_millis())
            .unwrap_or_else(|| Utc.from_utc_datetime(&ndt).timestamp_millis()),
    }
}

fn parse_date_clock(v: &Value, clock: NaiveClock) -> Option<i64> {
    if let Some(n) = v.as_f64() {
        if n > 1_000_000_000_000.0 {
            return Some(n as i64);
        }
        if n > 1_000_000_000.0 {
            return Some((n * 1000.0) as i64);
        }
    }
    if let Some(s) = v.as_str() {
        if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
            return Some(dt.timestamp_millis());
        }
        let fmts = [
            "%Y-%m-%dT%H:%M:%S",
            "%Y-%m-%dT%H:%M:%S%.f",
            "%Y-%m-%d %H:%M:%S",
            "%Y-%m-%d",
        ];
        for fmt in fmts {
            if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
                return Some(stamp_naive(naive, clock));
            }
            if let Ok(d) = chrono::NaiveDate::parse_from_str(s, fmt) {
                if let Some(ndt) = d.and_hms_opt(0, 0, 0) {
                    return Some(stamp_naive(ndt, clock));
                }
            }
        }
    }
    None
}

fn http_get_json(url: &str, headers: &[(&str, String)]) -> Result<(u16, Value), String> {
    let mut req = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?
        .get(url);
    for (k, v) in headers {
        req = req.header(*k, v);
    }
    let resp = req.send().map_err(|e| e.to_string())?;
    let code = resp.status().as_u16();
    let json = resp.json::<Value>().unwrap_or(json!({}));
    Ok((code, json))
}

fn http_post_json(url: &str, headers: &[(&str, String)], body: &Value) -> Result<(u16, Value), String> {
    let mut req = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?
        .post(url)
        .header("Content-Type", "application/json");
    for (k, v) in headers {
        req = req.header(*k, v);
    }
    let resp = req.json(body).send().map_err(|e| e.to_string())?;
    let code = resp.status().as_u16();
    let json = resp.json::<Value>().unwrap_or(json!({}));
    Ok((code, json))
}

fn http_post_form(url: &str, body: &str) -> Result<(u16, Value), String> {
    let resp = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?
        .post(url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body.to_string())
        .send()
        .map_err(|e| e.to_string())?;
    let code = resp.status().as_u16();
    let json = resp.json::<Value>().unwrap_or(json!({}));
    Ok((code, json))
}

fn empty(id: &str, title: &str, error: &str) -> ProviderSnapshot {
    ProviderSnapshot {
        id: id.into(),
        title: title.into(),
        headline_percent: None,
        metrics: vec![],
        error: Some(error.into()),
        updated_at: now_ms(),
        reset_notice: None,
    }
}

fn sqlite_query(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row(
        "SELECT value FROM ItemTable WHERE key = ? LIMIT 1",
        [key],
        |row| {
            if let Ok(s) = row.get::<_, String>(0) {
                return Ok(s);
            }
            let bytes: Vec<u8> = row.get(0)?;
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        },
    )
    .ok()
}

fn sqlite_value(db: &Path, key: &str) -> Option<String> {
    if !db.exists() {
        return None;
    }
    let ro = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY;
    if let Ok(conn) = Connection::open_with_flags(db, ro) {
        if let Some(v) = sqlite_query(&conn, key) {
            return Some(v);
        }
    }
    let flags = ro | rusqlite::OpenFlags::SQLITE_OPEN_URI;
    let encoded = db.to_string_lossy().replace(" ", "%20");
    let uri = format!("file:{encoded}?mode=ro");
    Connection::open_with_flags(&uri, flags)
        .ok()
        .and_then(|conn| sqlite_query(&conn, key))
}

// MARK: Codex

const CODEX_CLIENT: &str = "app_EMoamEEZ73f0CkXaXp7hrann";

fn fetch_codex() -> ProviderSnapshot {
    let auth_path = home().join(".codex/auth.json");
    let mut auth: Value = match fs::read_to_string(&auth_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
    {
        Some(v) => v,
        None => return empty("codex", "Codex Usage", "login_not_found"),
    };
    match pull_codex(&auth) {
        Ok(snap) => snap,
        Err(_) => {
            if refresh_codex(&mut auth, &auth_path) {
                pull_codex(&auth).unwrap_or_else(|e| empty("codex", "Codex Usage", &e))
            } else {
                empty("codex", "Codex Usage", "login_not_found")
            }
        }
    }
}

fn pull_codex(auth: &Value) -> Result<ProviderSnapshot, String> {
    let tokens = auth.get("tokens").ok_or("login_not_found")?;
    let access = tokens
        .get("access_token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or("login_not_found")?;
    let mut headers = vec![
        ("Authorization", format!("Bearer {access}")),
        ("Accept", "application/json".into()),
        ("User-Agent", "UsageBar/1.1".into()),
    ];
    if let Some(account) = tokens.get("account_id").and_then(|v| v.as_str()) {
        headers.push(("ChatGPT-Account-Id", account.into()));
    }
    let (code, json) = http_get_json("https://chatgpt.com/backend-api/wham/usage", &headers)?;
    if code == 401 {
        return Err("auth".into());
    }
    if code != 200 {
        return Err("api_error".into());
    }
    Ok(parse_codex(json))
}

fn parse_codex(json: Value) -> ProviderSnapshot {
    let mut metrics = vec![];
    if let Some(rate) = json.get("rate_limit") {
        metrics.extend(windows_from(rate, "plan", None));
    }
    if let Some(extras) = json.get("additional_rate_limits").and_then(|v| v.as_array()) {
        for extra in extras {
            let name = extra
                .get("limit_name")
                .and_then(|v| v.as_str())
                .unwrap_or("Extra");
            if let Some(rl) = extra.get("rate_limit") {
                metrics.extend(windows_from(rl, name, Some(name)));
            }
        }
    }
    let mut seen = HashSet::new();
    metrics.retain(|m| seen.insert(format!("{}{}{}", m.id, m.label, m.percent)));
    metrics.sort_by_key(|m| {
        if m.id == "plan-primary" {
            0
        } else if m.id == "plan-secondary" {
            1
        } else if m.id.starts_with("plan-") {
            2
        } else {
            3
        }
    });
    let headline = metrics
        .iter()
        .find(|m| m.id == "plan-primary" || m.label.contains("5-hour"))
        .or_else(|| metrics.first())
        .map(|m| m.percent);
    ProviderSnapshot {
        id: "codex".into(),
        title: "Codex Usage".into(),
        headline_percent: headline,
        error: if metrics.is_empty() {
            Some("no_quota".into())
        } else {
            None
        },
        metrics,
        updated_at: now_ms(),
        reset_notice: None,
    }
}

fn windows_from(rate: &Value, prefix: &str, label_prefix: Option<&str>) -> Vec<UsageMetric> {
    let mut out = vec![];
    if let Some(primary) = rate.get("primary_window") {
        if let Some(m) = metric_from(primary, &format!("{prefix}-primary"), label_prefix) {
            out.push(m);
        }
    }
    if let Some(secondary) = rate.get("secondary_window") {
        if let Some(m) = metric_from(secondary, &format!("{prefix}-secondary"), label_prefix) {
            out.push(m);
        }
    }
    out
}

fn metric_from(window: &Value, id: &str, extra: Option<&str>) -> Option<UsageMetric> {
    let percent = window.get("used_percent")?.as_f64()?;
    let seconds = window
        .get("limit_window_seconds")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let mut label = if seconds >= 2_400_000.0 {
        "Monthly limit"
    } else if seconds >= 400_000.0 {
        "Weekly limit"
    } else if seconds >= 10_000.0 {
        "5-hour window"
    } else {
        "Current window"
    }
    .to_string();
    if let Some(extra) = extra {
        if extra != "plan" {
            label = format!("{extra} · {label}");
        }
    }
    let reset = window
        .get("reset_at")
        .and_then(parse_date)
        .or_else(|| {
            window
                .get("reset_after_seconds")
                .and_then(|v| v.as_f64())
                .map(|after| now_ms() + (after * 1000.0) as i64)
        });
    Some(UsageMetric {
        id: id.into(),
        label,
        percent,
        resets_at: reset,
    })
}

fn refresh_codex(auth: &mut Value, path: &Path) -> bool {
    let Some(tokens) = auth.get_mut("tokens") else { return false };
    let Some(refresh) = tokens
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
    else {
        return false;
    };
    let body = format!(
        "grant_type=refresh_token&client_id={CODEX_CLIENT}&refresh_token={refresh}"
    );
    let Ok((200, json)) = http_post_form("https://auth.openai.com/oauth/token", &body) else {
        return false;
    };
    if let Some(obj) = tokens.as_object_mut() {
        if let Some(v) = json.get("access_token") {
            obj.insert("access_token".into(), v.clone());
        }
        if let Some(v) = json.get("refresh_token") {
            obj.insert("refresh_token".into(), v.clone());
        }
        if let Some(v) = json.get("id_token") {
            obj.insert("id_token".into(), v.clone());
        }
    }
    auth["last_refresh"] = json!(Utc::now().to_rfc3339());
    if let Ok(text) = serde_json::to_string_pretty(auth) {
        let _ = fs::write(path, text);
    }
    auth.get("tokens")
        .and_then(|t| t.get("access_token"))
        .and_then(|v| v.as_str())
        .is_some()
}

// MARK: Cursor

fn cursor_dbs() -> Vec<PathBuf> {
    let names = ["Cursor", "Cursor Nightly"];
    names
        .into_iter()
        .map(|name| {
            if cfg!(target_os = "macos") {
                home()
                    .join("Library/Application Support")
                    .join(name)
                    .join("User/globalStorage/state.vscdb")
            } else if cfg!(target_os = "windows") {
                dirs::data_dir()
                    .unwrap_or_else(|| home().join("AppData/Roaming"))
                    .join(name)
                    .join("User/globalStorage/state.vscdb")
            } else {
                dirs::config_dir()
                    .unwrap_or_else(|| home().join(".config"))
                    .join(name)
                    .join("User/globalStorage/state.vscdb")
            }
        })
        .collect()
}

fn clean_cursor_id(raw: String) -> Option<String> {
    let id = raw.trim().trim_matches('"').trim().to_string();
    if id.is_empty() || id.len() > 200 {
        None
    } else {
        Some(id)
    }
}

fn clean_cursor_token(raw: String) -> Option<String> {
    let token = raw.trim().trim_matches('"').trim().to_string();
    if token.is_empty() || token.len() > 16_384 {
        None
    } else {
        Some(token)
    }
}

fn jwt_sub(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let json = String::from_utf8(b64url_decode(payload)?).ok()?;
    serde_json::from_str::<Value>(&json)
        .ok()?
        .get("sub")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn b64url_decode(input: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u8> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        })
    }
    let bytes: Vec<u8> = input.bytes().filter(|c| *c != b'=').collect();
    if bytes.is_empty() {
        return Some(Vec::new());
    }
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let a = val(*chunk.first()?)?;
        let b = val(*chunk.get(1)?)?;
        out.push((a << 2) | (b >> 4));
        if chunk.len() >= 3 {
            let c = val(chunk[2])?;
            out.push(((b & 0x0f) << 4) | (c >> 2));
            if chunk.len() == 4 {
                let d = val(chunk[3])?;
                out.push(((c & 0x03) << 6) | d);
            }
        }
    }
    Some(out)
}

fn cursor_user_ids(db: &Path, token: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut push = |value: Option<String>| {
        if let Some(id) = value.and_then(clean_cursor_id) {
            if !ids.iter().any(|existing| existing == &id) {
                ids.push(id);
            }
        }
    };
    // Old Cursor writes cursorAuth/userId. Newer builds often omit it.
    push(sqlite_value(db, "cursorAuth/userId"));
    push(sqlite_value(db, "glass.lastSignedInAuthId"));
    push(sqlite_value(db, "adminSettings.cachedAuthId"));
    push(sqlite_value(db, "cursorAuth/stripeMembershipAuthId"));
    push(jwt_sub(token));
    ids
}

fn cursor_session_cookie(user_id: &str, token: &str) -> String {
    let user_id = user_id.replace('|', "%7C");
    format!("WorkosCursorSessionToken={user_id}%3A%3A{token}")
}

fn fetch_cursor() -> ProviderSnapshot {
    let mut last_error = "login_not_found".to_string();
    for db in cursor_dbs() {
        let Some(token) = sqlite_value(&db, "cursorAuth/accessToken").and_then(clean_cursor_token) else {
            continue;
        };
        let ids = cursor_user_ids(&db, &token);
        if ids.is_empty() {
            last_error = "login_not_found".into();
            continue;
        }
        for user_id in ids {
            let headers = [
                ("Accept", "application/json".into()),
                ("User-Agent", "Mozilla/5.0 UsageBar/1.0".into()),
                ("Origin", "https://cursor.com".into()),
                ("Referer", "https://cursor.com/dashboard?tab=usage".into()),
                ("Cookie", cursor_session_cookie(&user_id, &token)),
            ];
            match http_get_json("https://cursor.com/api/usage-summary", &headers) {
                Ok((200, json)) => {
                    let mut snap = parse_cursor(json);
                    if let Some(bot) = fetch_cursor_grok_bot(&headers, &token) {
                        attach_cursor_grok_bot(&mut snap, bot);
                    }
                    return snap;
                }
                Ok((code, _)) => last_error = format!("api_error:{code}"),
                Err(e) => last_error = e,
            }
        }
    }
    empty("cursor", "Cursor Usage", &last_error)
}

fn cursor_enabled(obj: &Value) -> bool {
    obj.get("enabled").and_then(|v| v.as_bool()) != Some(false)
}

fn cursor_amount_percent(obj: &Value) -> Option<f64> {
    let used = json_f64(obj.get("used"))?;
    let limit = json_f64(obj.get("limit"))?;
    if limit > 0.0 {
        Some(((used / limit) * 100.0).clamp(0.0, 100.0))
    } else {
        None
    }
}

fn parse_cursor(json: Value) -> ProviderSnapshot {
    let plan = json
        .pointer("/individualUsage/plan")
        .cloned()
        .unwrap_or(json!({}));
    let reset = json.get("billingCycleEnd").and_then(parse_date);
    let included = json_f64(plan.get("autoPercentUsed"));
    let api = json_f64(plan.get("apiPercentUsed"));
    let mut metrics = vec![];
    if let Some(included) = included {
        metrics.push(UsageMetric {
            id: "included".into(),
            label: "Included usage".into(),
            percent: included,
            resets_at: reset,
        });
    }
    if let Some(api) = api {
        metrics.push(UsageMetric {
            id: "api".into(),
            label: "API usage".into(),
            percent: api,
            resets_at: reset,
        });
    }
    if included.is_none() && api.is_none() {
        if let Some(overall) = json.pointer("/individualUsage/overall") {
            if cursor_enabled(overall) {
                if let Some(percent) = cursor_amount_percent(overall) {
                    metrics.push(UsageMetric {
                        id: "included".into(),
                        label: "Included usage".into(),
                        percent,
                        resets_at: reset,
                    });
                }
            }
        }
    }
    for path in ["/individualUsage/onDemand", "/teamUsage/onDemand"] {
        let Some(od) = json.pointer(path) else {
            continue;
        };
        if !cursor_enabled(od) || metrics.iter().any(|m| m.id == "ondemand") {
            continue;
        }
        if let Some(percent) = cursor_amount_percent(od) {
            metrics.push(UsageMetric {
                id: "ondemand".into(),
                label: "On-demand".into(),
                percent,
                resets_at: reset,
            });
        }
    }
    if let Some(pooled) = json.pointer("/teamUsage/pooled") {
        if cursor_enabled(pooled) {
            if let Some(percent) = cursor_amount_percent(pooled) {
                metrics.push(UsageMetric {
                    id: "pooled".into(),
                    label: "Team pooled".into(),
                    percent,
                    resets_at: reset,
                });
            }
        }
    }
    if let Some(bot) = parse_cursor_grok_bot(&json) {
        if !metrics.iter().any(|m| m.id == "grok-bot") {
            metrics.push(bot);
        }
    }
    ProviderSnapshot {
        id: "cursor".into(),
        title: "Cursor Usage".into(),
        headline_percent: included
            .or(api)
            .or_else(|| metrics.first().map(|m| m.percent)),
        error: if metrics.is_empty() {
            Some("no_quota".into())
        } else {
            None
        },
        metrics,
        updated_at: now_ms(),
        reset_notice: None,
    }
}

fn attach_cursor_grok_bot(snap: &mut ProviderSnapshot, bot: UsageMetric) {
    if snap.metrics.iter().any(|m| m.id == "grok-bot") {
        return;
    }
    snap.metrics.push(bot);
    if !snap.metrics.is_empty() {
        snap.error = None;
        if snap.headline_percent.is_none() {
            snap.headline_percent = snap.metrics.first().map(|m| m.percent);
        }
    }
}

fn fetch_cursor_grok_bot(headers: &[(&str, String)], token: &str) -> Option<UsageMetric> {
    if let Ok((200, json)) = http_post_json(
        "https://cursor.com/api/dashboard/get-sand-usage-status",
        headers,
        &json!({}),
    ) {
        if let Some(metric) = parse_cursor_grok_bot(&json) {
            return Some(metric);
        }
    }
    let connect = [
        ("Authorization", format!("Bearer {token}")),
        ("Accept", "application/json".into()),
        ("Connect-Protocol-Version", "1".into()),
        ("User-Agent", "Mozilla/5.0 UsageBar/1.0".into()),
    ];
    if let Ok((200, json)) = http_post_json(
        "https://api2.cursor.sh/aiserver.v1.DashboardService/GetSandUsageStatus",
        &connect,
        &json!({}),
    ) {
        return parse_cursor_grok_bot(&json);
    }
    None
}

fn parse_cursor_grok_bot(json: &Value) -> Option<UsageMetric> {
    let obj = json
        .get("status")
        .or_else(|| json.get("sandUsage"))
        .or_else(|| json.get("grokBot"))
        .or_else(|| json.pointer("/individualUsage/grokBot"))
        .unwrap_or(json);
    let has_limit = obj
        .get("hasNonZeroIncludedLimit")
        .or_else(|| obj.get("has_non_zero_included_limit"))
        .and_then(|v| v.as_bool());
    if has_limit != Some(true) {
        return None;
    }
    let percent = json_f64(obj.get("usagePercent"))
        .or_else(|| json_f64(obj.get("usage_percent")))
        .or_else(|| json_f64(obj.get("includedUsagePercent")))?;
    let reset = obj
        .get("nextResetTimestampUtc")
        .or_else(|| obj.get("next_reset_timestamp_utc"))
        .or_else(|| obj.get("resetsAt"))
        .or_else(|| obj.get("resets_at"))
        .and_then(parse_date);
    Some(UsageMetric {
        id: "grok-bot".into(),
        label: "Grok Bot · Weekly limit".into(),
        percent,
        resets_at: reset,
    })
}

// MARK: Grok

fn fetch_grok() -> ProviderSnapshot {
    let Some((token, refresh, user_id, issuer, client_id)) = load_grok() else {
        return empty("grok", "Grok Usage", "login_not_found");
    };
    let mut headers = vec![
        ("Authorization", format!("Bearer {token}")),
        ("Accept", "application/json".into()),
        ("X-XAI-Token-Auth", "xai-grok-cli".into()),
        ("x-grok-client-mode", "interactive".into()),
        ("x-grok-client-version", grok_version()),
    ];
    if let Some(user) = &user_id {
        headers.push(("x-userid", user.clone()));
    }
    let urls = [
        "https://cli-chat-proxy.grok.com/v1/billing?format=credits",
        "https://cli-chat-proxy.grok.com/v1/billing",
    ];
    let mut last_error = "api_error".to_string();
    let mut bodies = Vec::new();
    for url in urls {
        let mut result = http_get_json(url, &headers);
        if let Ok((401, _)) = &result {
            if let Some(new_token) =
                refresh_grok(issuer.as_deref(), refresh.as_deref(), client_id.as_deref())
            {
                headers[0] = ("Authorization", format!("Bearer {new_token}"));
                result = http_get_json(url, &headers);
            }
        }
        match result {
            Ok((200, json)) => bodies.push(json),
            Ok((code, _)) => last_error = format!("api_error:{code}"),
            Err(e) => last_error = e,
        }
    }
    if bodies.is_empty() {
        return empty("grok", "Grok Usage", &last_error);
    }
    parse_grok_bodies(bodies)
}

fn money_val(v: Option<&Value>) -> Option<f64> {
    let v = v?;
    json_f64(v.get("val")).or_else(|| json_f64(Some(v)))
}

fn parse_grok_bodies(bodies: Vec<Value>) -> ProviderSnapshot {
    let mut metrics = vec![];
    let mut reset = None;
    for json in bodies {
        let config = json.get("config").cloned().unwrap_or(json);
        if reset.is_none() {
            reset = config
                .pointer("/currentPeriod/end")
                .and_then(parse_date)
                .or_else(|| config.get("billingPeriodEnd").and_then(parse_date));
        }
        if let Some(weekly) = json_f64(config.get("creditUsagePercent")) {
            if !metrics.iter().any(|m: &UsageMetric| m.id == "weekly") {
                metrics.push(UsageMetric {
                    id: "weekly".into(),
                    label: "Weekly allowance".into(),
                    percent: weekly,
                    resets_at: reset,
                });
            }
        }
        if let Some(products) = config.get("productUsage").and_then(|v| v.as_array()) {
            for product in products {
                let name = product
                    .get("product")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Product");
                let Some(pct) = json_f64(product.get("usagePercent")) else {
                    continue;
                };
                if metrics.iter().any(|m| m.id == name) {
                    continue;
                }
                let label = match name {
                    "GrokBuild" => "Grok Build",
                    "GrokChat" => "Grok Chat",
                    other => other,
                };
                metrics.push(UsageMetric {
                    id: name.into(),
                    label: label.into(),
                    percent: pct,
                    resets_at: reset,
                });
            }
        }
        if let (Some(used), Some(limit)) = (
            money_val(config.get("used")),
            money_val(config.get("monthlyLimit")),
        ) {
            if limit > 0.0 && !metrics.iter().any(|m| m.id == "monthly") {
                let pct = ((used / limit) * 100.0).clamp(0.0, 100.0);
                metrics.push(UsageMetric {
                    id: "monthly".into(),
                    label: "Monthly limit".into(),
                    percent: pct,
                    resets_at: reset,
                });
            }
        }
        if let (Some(used), Some(cap)) = (
            money_val(config.get("onDemandUsed")),
            money_val(config.get("onDemandCap")),
        ) {
            if cap > 0.0 && !metrics.iter().any(|m| m.id == "ondemand") {
                let pct = ((used / cap) * 100.0).clamp(0.0, 100.0);
                metrics.push(UsageMetric {
                    id: "ondemand".into(),
                    label: "On-demand".into(),
                    percent: pct,
                    resets_at: reset,
                });
            }
        }
    }
    let headline = metrics
        .iter()
        .find(|m| m.id == "weekly")
        .or_else(|| metrics.iter().find(|m| m.id == "monthly"))
        .or_else(|| metrics.first())
        .map(|m| m.percent);
    ProviderSnapshot {
        id: "grok".into(),
        title: "Grok Usage".into(),
        headline_percent: headline,
        error: if metrics.is_empty() {
            Some("no_quota".into())
        } else {
            None
        },
        metrics,
        updated_at: now_ms(),
        reset_notice: None,
    }
}

fn grok_version() -> String {
    fs::read_to_string(home().join(".grok/.metadata_version"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "1.0.5".into())
}

fn parse_grok_auth(obj: &Value) -> Option<(String, Option<String>, Option<String>, Option<String>, Option<String>)> {
    if let Some(key) = obj.get("key").and_then(|v| v.as_str()) {
        if obj.get("auth_mode").and_then(|v| v.as_str()) != Some("api_key") {
            return Some((
                key.into(),
                obj.get("refresh_token").and_then(|v| v.as_str()).map(str::to_string),
                obj.get("user_id").and_then(|v| v.as_str()).map(str::to_string),
                obj.get("oidc_issuer").and_then(|v| v.as_str()).map(str::to_string),
                obj.get("oidc_client_id").and_then(|v| v.as_str()).map(str::to_string),
            ));
        }
    }
    if let Some(map) = obj.as_object() {
        for (k, v) in map {
            let Some(entry) = v.as_object() else { continue };
            let Some(key) = entry.get("key").and_then(|x| x.as_str()) else { continue };
            if entry.get("auth_mode").and_then(|x| x.as_str()) == Some("api_key") {
                continue;
            }
            return Some((
                key.into(),
                entry.get("refresh_token").and_then(|x| x.as_str()).map(str::to_string),
                entry.get("user_id").and_then(|x| x.as_str()).map(str::to_string),
                entry
                    .get("oidc_issuer")
                    .and_then(|x| x.as_str())
                    .map(str::to_string)
                    .or_else(|| k.split(':').next().map(|s| s.to_string())),
                entry.get("oidc_client_id").and_then(|x| x.as_str()).map(str::to_string),
            ));
        }
    }
    None
}

fn load_grok() -> Option<(String, Option<String>, Option<String>, Option<String>, Option<String>)> {
    if let Ok(override_path) = std::env::var("GROK_AUTH_JSON") {
        if let Ok(data) = fs::read_to_string(override_path) {
            if let Ok(obj) = serde_json::from_str::<Value>(&data) {
                if let Some(c) = parse_grok_auth(&obj) {
                    return Some(c);
                }
            }
        }
    }
    let grok_home = std::env::var("GROK_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home().join(".grok"));
    let data = fs::read_to_string(grok_home.join("auth.json")).ok()?;
    let obj = serde_json::from_str(&data).ok()?;
    parse_grok_auth(&obj)
}

fn refresh_grok(issuer: Option<&str>, refresh: Option<&str>, client: Option<&str>) -> Option<String> {
    let issuer = issuer?;
    let refresh = refresh?;
    let client = client?;
    let well_known = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );
    let (_, wk) = http_get_json(&well_known, &[]).ok()?;
    let token_url = wk.get("token_endpoint")?.as_str()?;
    let body = format!("grant_type=refresh_token&refresh_token={refresh}&client_id={client}");
    let (200, json) = http_post_form(token_url, &body).ok()? else {
        return None;
    };
    json.get("access_token")?.as_str().map(|s| s.to_string())
}

// MARK: GLM

fn fetch_glm() -> ProviderSnapshot {
    let Some(key) = glm_key() else {
        return empty("glm", "GLM Usage", "login_not_found");
    };
    fetch_zhipu_quota("glm", "GLM Usage", &key, &["https://api.z.ai", "https://open.bigmodel.cn"])
}

fn fetch_zcode() -> ProviderSnapshot {
    let Some((key, host)) = zcode_coding_plan() else {
        return empty("zcode", "ZCode Usage", "login_not_found");
    };
    let mut hosts = vec![host];
    if host.contains("bigmodel") {
        hosts.push("https://api.z.ai");
    } else {
        hosts.push("https://open.bigmodel.cn");
    }
    fetch_zhipu_quota("zcode", "ZCode Usage", &key, &hosts)
}

fn fetch_zhipu_quota(id: &str, title: &str, key: &str, hosts: &[&str]) -> ProviderSnapshot {
    let headers = [
        ("Authorization", format!("Bearer {key}")),
        ("Accept", "application/json".into()),
    ];
    for host in hosts {
        let url = format!("{host}/api/monitor/usage/quota/limit");
        if let Ok((200, json)) = http_get_json(&url, &headers) {
            return parse_glm(id, title, json);
        }
    }
    empty(id, title, "login_not_found")
}

fn glm_limit_label(typ: &str, unit: i64, number: i64) -> String {
    if typ == "TIME_LIMIT" {
        return "MCP tools".into();
    }
    match unit {
        3 if number == 5 => "5-hour window".into(),
        3 if number > 0 => format!("{number}-hour window"),
        6 if number == 30 || number >= 28 => "Monthly limit".into(),
        6 if number == 7 || number == 1 => "Weekly limit".into(),
        6 if number > 0 => format!("{number}-day window"),
        _ if typ.is_empty() => "Usage".into(),
        _ => typ.into(),
    }
}

fn glm_item_percent(item: &Value) -> Option<f64> {
    if let Some(percent) = json_f64(item.get("percentage")) {
        return Some(percent);
    }
    let used = json_f64(item.get("currentValue"))?;
    let limit = json_f64(item.get("usage"))?;
    if limit > 0.0 {
        Some(((used / limit) * 100.0).clamp(0.0, 100.0))
    } else {
        None
    }
}

fn parse_glm(id: &str, title: &str, json: Value) -> ProviderSnapshot {
    let data = json.get("data").cloned().unwrap_or(json);
    let limits = data
        .get("limits")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut metrics = vec![];
    for item in limits {
        let Some(percent) = glm_item_percent(&item) else {
            continue;
        };
        let typ = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let unit = item.get("unit").and_then(|v| v.as_i64()).unwrap_or(0);
        let number = item.get("number").and_then(|v| v.as_i64()).unwrap_or(0);
        let reset = item.get("nextResetTime").and_then(parse_date_beijing);
        metrics.push(UsageMetric {
            id: format!("{typ}-{unit}-{number}"),
            label: glm_limit_label(typ, unit, number),
            percent,
            resets_at: reset,
        });
    }
    metrics.sort_by_key(|m| {
        if m.label == "5-hour window" || m.label.ends_with("-hour window") {
            0
        } else if m.label == "Weekly limit" {
            1
        } else if m.label == "Monthly limit" {
            2
        } else if m.label == "MCP tools" {
            3
        } else if m.label.ends_with("-day window") {
            4
        } else {
            9
        }
    });
    let headline = metrics
        .iter()
        .find(|m| m.label == "5-hour window")
        .or_else(|| metrics.iter().find(|m| m.label == "Weekly limit"))
        .or_else(|| metrics.iter().find(|m| m.label == "Monthly limit"))
        .or_else(|| metrics.first())
        .map(|m| m.percent);
    ProviderSnapshot {
        id: id.into(),
        title: title.into(),
        headline_percent: headline,
        error: if metrics.is_empty() {
            Some("no_quota".into())
        } else {
            None
        },
        metrics,
        updated_at: now_ms(),
        reset_notice: None,
    }
}

fn glm_key() -> Option<String> {
    for key in ["GLM_API_KEY", "ZAI_API_KEY", "Z_AI_API_KEY"] {
        if let Some(v) = env_or_shell(key) {
            return Some(v);
        }
    }
    if let Ok(data) = fs::read_to_string(home().join(".zai/config.json")) {
        if let Ok(obj) = serde_json::from_str::<Value>(&data) {
            if let Some(k) = obj
                .get("apiKey")
                .or_else(|| obj.get("api_key"))
                .and_then(|v| v.as_str())
            {
                return Some(k.into());
            }
        }
    }
    cc_switch_glm_key().or_else(|| zcode_coding_plan().map(|(key, _)| key))
}

fn zcode_coding_plan() -> Option<(String, &'static str)> {
    let path = home().join(".zcode/v2/config.json");
    let obj = serde_json::from_str::<Value>(&fs::read_to_string(path).ok()?).ok()?;
    zcode_coding_plan_from(&obj)
}

fn zcode_coding_plan_from(obj: &Value) -> Option<(String, &'static str)> {
    let providers = obj.get("provider")?.as_object()?;
    const PREFERRED: &[&str] = &[
        "builtin:bigmodel-coding-plan",
        "builtin:zai-coding-plan",
        "builtin:bigmodel-start-plan",
        "builtin:zai-start-plan",
        "builtin:bigmodel",
        "builtin:zai",
    ];
    for id in PREFERRED {
        if let Some(pair) = zcode_provider_key(id, providers.get(*id)?) {
            return Some(pair);
        }
    }
    for (id, provider) in providers {
        if PREFERRED.contains(&id.as_str()) {
            continue;
        }
        if let Some(pair) = zcode_provider_key(id, provider) {
            return Some(pair);
        }
    }
    None
}

fn zcode_provider_key(id: &str, provider: &Value) -> Option<(String, &'static str)> {
    if provider.get("enabled").and_then(|v| v.as_bool()) == Some(false) {
        return None;
    }
    let key = provider
        .pointer("/options/apiKey")
        .or_else(|| provider.pointer("/options/api_key"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| s.len() > 8)?;
    Some((key.to_string(), zcode_host(id, provider)))
}

fn zcode_host(id: &str, provider: &Value) -> &'static str {
    let base = provider
        .pointer("/options/baseURL")
        .or_else(|| provider.pointer("/options/base_url"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if base.contains("z.ai") || id.contains("zai") {
        "https://api.z.ai"
    } else {
        "https://open.bigmodel.cn"
    }
}

fn cc_switch_glm_key() -> Option<String> {
    let db = home().join(".cc-switch/cc-switch.db");
    let conn = Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    let mut stmt = conn
        .prepare("SELECT name, settings_config FROM providers")
        .ok()?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .ok()?;
    for row in rows.flatten() {
        let (name, config) = row;
        let interesting = name.to_lowercase().contains("glm")
            || name.contains("智谱")
            || name.to_lowercase().contains("z.ai")
            || name.to_lowercase().contains("zhipu");
        if !interesting {
            continue;
        }
        let Ok(obj) = serde_json::from_str::<Value>(&config) else {
            continue;
        };
        if let Some(env) = obj.get("env") {
            if let Some(token) = env
                .get("ANTHROPIC_AUTH_TOKEN")
                .or_else(|| env.get("ANTHROPIC_API_KEY"))
                .and_then(|v| v.as_str())
            {
                if token.len() > 20 {
                    return Some(token.into());
                }
            }
        }
    }
    None
}

pub fn fetch_selected_each(
    ids: &[String],
    mut on_each: impl FnMut(&ProviderSnapshot),
) -> Vec<ProviderSnapshot> {
    let mut snaps = Vec::new();
    for id in crate::prefs::normalize_visible(ids) {
        let mut one = vec![fetch_one(&id)];
        crate::usage_state::apply(&mut one);
        let snap = one.remove(0);
        on_each(&snap);
        snaps.push(snap);
    }
    snaps
}

fn fetch_one(id: &str) -> ProviderSnapshot {
    match id {
        "codex" => fetch_codex(),
        "cursor" => fetch_cursor(),
        "grok" => fetch_grok(),
        "glm" => fetch_glm(),
        "zcode" => fetch_zcode(),
        "claude" => fetch_claude(),
        "copilot" => fetch_copilot(),
        "gemini" => fetch_gemini(),
        "antigravity" => fetch_antigravity(),
        other => empty(other, "Usage", "login_not_found"),
    }
}

fn claude_version() -> String {
    for path in [
        home().join(".claude/version"),
        home().join(".claude/.version"),
    ] {
        if let Ok(text) = fs::read_to_string(path) {
            let v = text.trim();
            if !v.is_empty() {
                return v.to_string();
            }
        }
    }
    "2.1.72".into()
}

fn claude_oauth() -> Option<(String, Option<String>, Option<PathBuf>)> {
    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = std::process::Command::new("security")
            .args(["find-generic-password", "-s", "Claude Code-credentials", "-w"])
            .output()
        {
            if out.status.success() {
                if let Ok(text) = String::from_utf8(out.stdout) {
                    if let Ok(obj) = serde_json::from_str::<Value>(text.trim()) {
                        if let Some(pair) = claude_tokens(&obj) {
                            return Some((pair.0, pair.1, None));
                        }
                    }
                }
            }
        }
    }
    for path in [
        home().join(".claude/.credentials.json"),
        home().join(".claude/credentials.json"),
        home().join(".config/claude/credentials.json"),
    ] {
        if let Ok(data) = fs::read_to_string(&path) {
            if let Ok(obj) = serde_json::from_str::<Value>(&data) {
                if let Some(pair) = claude_tokens(&obj) {
                    return Some((pair.0, pair.1, Some(path)));
                }
            }
        }
    }
    env_or_shell("CLAUDE_ACCESS_TOKEN").map(|t| (t, None, None))
}

fn claude_tokens(obj: &Value) -> Option<(String, Option<String>)> {
    let oauth = obj.get("claudeAiOauth").unwrap_or(obj);
    let access = oauth
        .get("accessToken")
        .or_else(|| oauth.get("access_token"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())?;
    let refresh = oauth
        .get("refreshToken")
        .or_else(|| oauth.get("refresh_token"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    Some((access.to_string(), refresh))
}

fn fetch_claude() -> ProviderSnapshot {
    let Some((mut token, refresh, path)) = claude_oauth() else {
        return empty("claude", "Claude Usage", "login_not_found");
    };
    match pull_claude(&token) {
        Ok(snap) => snap,
        Err(_) => {
            if let Some(new_token) = refresh.as_deref().and_then(refresh_claude) {
                if let Some(path) = path {
                    if let Ok(data) = fs::read_to_string(&path) {
                        if let Ok(mut obj) = serde_json::from_str::<Value>(&data) {
                            if obj.get("claudeAiOauth").is_some() {
                                obj["claudeAiOauth"]["accessToken"] = json!(new_token);
                            } else {
                                obj["accessToken"] = json!(new_token);
                            }
                            let _ = fs::write(path, serde_json::to_string_pretty(&obj).unwrap_or_default());
                        }
                    }
                }
                token = new_token;
                pull_claude(&token).unwrap_or_else(|e| empty("claude", "Claude Usage", &e))
            } else {
                empty("claude", "Claude Usage", "auth")
            }
        }
    }
}

fn refresh_claude(refresh: &str) -> Option<String> {
    let body = format!(
        "grant_type=refresh_token&refresh_token={refresh}&client_id=9d1c250a-e61b-44d9-88ed-5944d1962f5e"
    );
    let (200, json) = http_post_form("https://console.anthropic.com/v1/oauth/token", &body).ok()? else {
        return None;
    };
    json.get("access_token")
        .or_else(|| json.get("accessToken"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn pull_claude(token: &str) -> Result<ProviderSnapshot, String> {
    let headers = [
        ("Authorization", format!("Bearer {token}")),
        ("anthropic-beta", "oauth-2025-04-20".into()),
        ("Content-Type", "application/json".into()),
        ("User-Agent", format!("claude-code/{}", claude_version())),
    ];
    let (code, json) = http_get_json("https://api.anthropic.com/api/oauth/usage", &headers)?;
    if code == 401 || code == 403 {
        return Err("auth".into());
    }
    if code != 200 {
        return Err("api_error".into());
    }
    Ok(parse_claude(json))
}

fn claude_scoped_weekly(name: &str) -> String {
    match name {
        "Opus" | "Claude Opus" => "Weekly Opus".into(),
        "Sonnet" | "Claude Sonnet" => "Weekly Sonnet".into(),
        other => format!("{other} · Weekly limit"),
    }
}

fn money_amount(v: Option<&Value>) -> Option<f64> {
    let v = v?;
    if let Some(n) = json_f64(Some(v)) {
        return Some(n);
    }
    let minor = json_f64(v.get("amount_minor"))?;
    let exp = json_f64(v.get("exponent")).unwrap_or(2.0) as i32;
    Some(minor / 10f64.powi(exp.max(0)))
}

fn claude_extra_usage(json: &Value) -> Option<UsageMetric> {
    if let Some(extra) = json.get("extra_usage") {
        if extra.get("is_enabled").and_then(|v| v.as_bool()) != Some(false) {
            let limit = json_f64(extra.get("monthly_limit"));
            let used = json_f64(extra.get("used_credits")).or_else(|| json_f64(extra.get("used")));
            let percent = json_f64(extra.get("utilization")).or_else(|| match (used, limit) {
                (Some(u), Some(l)) if l > 0.0 => Some(((u / l) * 100.0).clamp(0.0, 100.0)),
                _ => None,
            });
            if let Some(percent) = percent {
                if limit.unwrap_or(1.0) > 0.0 {
                    return Some(UsageMetric {
                        id: "extra_usage".into(),
                        label: "Monthly limit".into(),
                        percent,
                        resets_at: extra
                            .get("resets_at")
                            .and_then(parse_date)
                            .or_else(|| extra.get("reset_at").and_then(parse_date)),
                    });
                }
            }
        }
    }
    let spend = json.get("spend")?;
    if spend.get("enabled").and_then(|v| v.as_bool()) == Some(false) {
        return None;
    }
    let used = money_amount(spend.get("used"));
    let limit = money_amount(spend.get("limit"));
    let percent = json_f64(spend.get("percent"))
        .or_else(|| json_f64(spend.get("utilization")))
        .or_else(|| match (used, limit) {
            (Some(u), Some(l)) if l > 0.0 => Some(((u / l) * 100.0).clamp(0.0, 100.0)),
            _ => None,
        })?;
    Some(UsageMetric {
        id: "spend".into(),
        label: "Monthly limit".into(),
        percent,
        resets_at: spend.get("resets_at").and_then(parse_date),
    })
}

fn parse_claude(json: Value) -> ProviderSnapshot {
    let mut metrics = vec![];
    if let Some(limits) = json.get("limits").and_then(|v| v.as_array()) {
        for item in limits {
            let percent = json_f64(item.get("percent")).or_else(|| json_f64(item.get("utilization")));
            let Some(percent) = percent else { continue };
            let kind = item.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            let scoped = item
                .pointer("/scope/model/display_name")
                .and_then(|v| v.as_str());
            let label = match kind {
                "session" => "5-hour window".into(),
                "weekly_all" => "Weekly limit".into(),
                "weekly_opus" => "Weekly Opus".into(),
                "weekly_sonnet" => "Weekly Sonnet".into(),
                "weekly_scoped" => scoped.map(claude_scoped_weekly).unwrap_or_else(|| "Weekly limit".into()),
                other if !other.is_empty() => {
                    if let Some(name) = scoped {
                        format!("{name} · {other}")
                    } else {
                        other.to_string()
                    }
                }
                _ => scoped.unwrap_or("Usage").to_string(),
            };
            let id = if kind == "weekly_scoped" {
                format!("weekly_scoped-{}", scoped.unwrap_or("model"))
            } else {
                kind.to_string()
            };
            metrics.push(UsageMetric {
                id,
                label,
                percent,
                resets_at: item.get("resets_at").and_then(parse_date),
            });
        }
    }
    if metrics.is_empty() {
        for (key, label) in [
            ("five_hour", "5-hour window"),
            ("seven_day", "Weekly limit"),
            ("seven_day_opus", "Weekly Opus"),
            ("seven_day_sonnet", "Weekly Sonnet"),
        ] {
            if let Some(win) = json.get(key) {
                if let Some(percent) = json_f64(win.get("utilization")) {
                    metrics.push(UsageMetric {
                        id: key.into(),
                        label: label.into(),
                        percent,
                        resets_at: win.get("resets_at").and_then(parse_date),
                    });
                }
            }
        }
    }
    if let Some(extra) = claude_extra_usage(&json) {
        if !metrics.iter().any(|m| m.id == "extra_usage" || m.id == "spend" || m.label == "Monthly limit")
        {
            metrics.push(extra);
        }
    }
    ProviderSnapshot {
        id: "claude".into(),
        title: "Claude Usage".into(),
        headline_percent: metrics
            .iter()
            .find(|m| m.id == "session" || m.id == "five_hour")
            .or_else(|| metrics.first())
            .map(|m| m.percent),
        error: if metrics.is_empty() {
            Some("no_quota".into())
        } else {
            None
        },
        metrics,
        updated_at: now_ms(),
        reset_notice: None,
    }
}

fn copilot_token() -> Option<String> {
    let dir = if cfg!(target_os = "windows") {
        dirs::data_dir()
            .unwrap_or_else(|| home().join("AppData/Local"))
            .join("github-copilot")
    } else {
        home().join(".config/github-copilot")
    };
    for name in ["apps.json", "hosts.json"] {
        if let Some(token) = copilot_token_from_json(&dir.join(name)) {
            return Some(token);
        }
    }
    if let Some(token) = copilot_token_from_gh(&home().join(".config/gh/hosts.yml")) {
        return Some(token);
    }
    env_or_shell("GITHUB_TOKEN").or_else(|| env_or_shell("GH_TOKEN"))
}

fn copilot_token_from_json(path: &Path) -> Option<String> {
    let obj = serde_json::from_str::<Value>(&fs::read_to_string(path).ok()?).ok()?;
    if let Some(token) = obj.get("oauth_token").and_then(|v| v.as_str()) {
        if !token.is_empty() {
            return Some(token.into());
        }
    }
    if let Some(map) = obj.as_object() {
        for (host, entry) in map {
            if !host.contains("github.com") {
                continue;
            }
            if let Some(token) = entry.get("oauth_token").and_then(|v| v.as_str()) {
                if !token.is_empty() {
                    return Some(token.into());
                }
            }
        }
        for entry in map.values() {
            if let Some(token) = entry.get("oauth_token").and_then(|v| v.as_str()) {
                if !token.is_empty() {
                    return Some(token.into());
                }
            }
        }
    }
    None
}

fn copilot_token_from_gh(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let mut in_github = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if !line.starts_with(' ') && !line.starts_with('\t') && trimmed.ends_with(':') {
            in_github = trimmed.trim_end_matches(':').contains("github.com");
        }
        if in_github && trimmed.starts_with("oauth_token:") {
            let token = trimmed.trim_start_matches("oauth_token:").trim();
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    None
}

fn fetch_copilot() -> ProviderSnapshot {
    let Some(token) = copilot_token() else {
        return empty("copilot", "Copilot Usage", "login_not_found");
    };
    let headers = [
        ("Authorization", format!("Bearer {token}")),
        ("Accept", "application/json".into()),
        ("Editor-Version", "vscode/1.96.0".into()),
        ("User-Agent", "UsageBar/1.0".into()),
    ];
    let json = match http_get_json("https://api.github.com/copilot_internal/user", &headers) {
        Ok((200, json)) => json,
        Ok((401 | 403, _)) => return empty("copilot", "Copilot Usage", "auth"),
        Ok((code, _)) => return empty("copilot", "Copilot Usage", &format!("api_error:{code}")),
        Err(e) => return empty("copilot", "Copilot Usage", &e),
    };
    parse_copilot(json)
}

fn copilot_snapshot_percent(snap: &Value) -> Option<f64> {
    if snap.get("unlimited").and_then(|v| v.as_bool()) == Some(true) {
        return None;
    }
    if let Some(left) = json_f64(snap.get("percent_remaining")) {
        return Some((100.0 - left).clamp(0.0, 100.0));
    }
    let entitlement = json_f64(snap.get("entitlement")).unwrap_or(0.0);
    let remaining = json_f64(snap.get("remaining")).unwrap_or(0.0);
    if entitlement > 0.0 {
        Some(((entitlement - remaining).max(0.0) / entitlement * 100.0).clamp(0.0, 100.0))
    } else {
        None
    }
}

fn copilot_snapshot_label(key: &str) -> String {
    match key {
        "premium_interactions" => "Premium requests".into(),
        "chat" | "chat_interactions" => "Chat".into(),
        "completions" => "Completions".into(),
        other => other.to_string(),
    }
}

fn parse_copilot(json: Value) -> ProviderSnapshot {
    let reset = json
        .get("quota_reset_date_utc")
        .and_then(parse_date)
        .or_else(|| json.get("quota_reset_date").and_then(parse_date));
    let mut metrics = vec![];
    if let Some(map) = json.get("quota_snapshots").and_then(|v| v.as_object()) {
        let mut keys: Vec<String> = map.keys().cloned().collect();
        keys.sort_by_key(|k| match k.as_str() {
            "premium_interactions" => 0,
            "chat" | "chat_interactions" => 1,
            "completions" => 2,
            _ => 9,
        });
        for key in keys {
            let Some(percent) = copilot_snapshot_percent(&map[&key]) else {
                continue;
            };
            metrics.push(UsageMetric {
                id: key.clone(),
                label: copilot_snapshot_label(&key),
                percent,
                resets_at: reset,
            });
        }
    }
    ProviderSnapshot {
        id: "copilot".into(),
        title: "Copilot Usage".into(),
        headline_percent: metrics.first().map(|m| m.percent),
        error: if metrics.is_empty() {
            Some("no_quota".into())
        } else {
            None
        },
        metrics,
        updated_at: now_ms(),
        reset_notice: None,
    }
}

fn google_installed_client(prefix: &str, mid: &str) -> String {
    format!("{prefix}-{mid}.apps.googleusercontent.com")
}

fn google_installed_secret(rest: &str) -> String {
    format!("{}{rest}", ["GOC", "SPX-"].concat())
}

fn gemini_oauth_client() -> String {
    google_installed_client("681255809395", "oo8ft2oprdrnp9e3aqf6av3hmdib135j")
}

fn gemini_oauth_secret() -> String {
    google_installed_secret("4uHgM2Xoy7vWl25n0Moh6-5kq3jP")
}

fn antigravity_oauth_client() -> String {
    google_installed_client("1071006060591", "tmhssin2h21lcre235vtolojh4g403ep")
}

fn antigravity_oauth_secret() -> String {
    google_installed_secret("K58FWR486LdLJ1mLB8sXC4z6qDAf")
}

fn refresh_google(refresh: &str, client_id: &str, client_secret: &str) -> Option<String> {
    let body = format!(
        "client_id={client_id}&client_secret={client_secret}&refresh_token={refresh}&grant_type=refresh_token"
    );
    let (200, json) = http_post_form("https://oauth2.googleapis.com/token", &body).ok()? else {
        return None;
    };
    json.get("access_token")?.as_str().map(str::to_string)
}

fn gemini_access_token() -> Option<String> {
    let paths = [
        home().join(".gemini/oauth_creds.json"),
        home().join(".config/gemini/oauth_creds.json"),
    ];
    for path in paths {
        let Ok(data) = fs::read_to_string(&path) else { continue };
        let Ok(mut obj) = serde_json::from_str::<Value>(&data) else { continue };
        let access = obj
            .get("access_token")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let expiry = obj.get("expiry_date").and_then(|v| v.as_i64()).unwrap_or(0);
        let refresh = obj
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        if !access.is_empty() && (expiry == 0 || expiry > now_ms() + 30_000) {
            return Some(access);
        }
        if let Some(refresh) = refresh {
            if let Some(token) = refresh_google(&refresh, &gemini_oauth_client(), &gemini_oauth_secret()) {
                obj["access_token"] = json!(token);
                obj["expiry_date"] = json!(now_ms() + 3_000_000);
                let _ = fs::write(path, serde_json::to_string_pretty(&obj).unwrap_or_default());
                return Some(token);
            }
        }
        if !access.is_empty() {
            return Some(access);
        }
    }
    None
}

fn fetch_gemini() -> ProviderSnapshot {
    let Some(token) = gemini_access_token() else {
        return empty("gemini", "Gemini Usage", "login_not_found");
    };
    let headers = [
        ("Authorization", format!("Bearer {token}")),
        ("Content-Type", "application/json".into()),
    ];
    let load_body = json!({
        "metadata": {
            "ideType": "IDE_UNSPECIFIED",
            "platform": "PLATFORM_UNSPECIFIED",
            "pluginType": "GEMINI"
        }
    });
    let json = match http_post_json(
        "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
        &headers,
        &load_body,
    ) {
        Ok((200, json)) => json,
        Ok((401 | 403, _)) => return empty("gemini", "Gemini Usage", "auth"),
        Ok((code, _)) => return empty("gemini", "Gemini Usage", &format!("api_error:{code}")),
        Err(e) => return empty("gemini", "Gemini Usage", &e),
    };
    let project = json
        .get("cloudaicompanionProject")
        .and_then(|v| v.as_str())
        .or_else(|| json.pointer("/cloudaicompanionProject/id").and_then(|v| v.as_str()));
    let Some(project) = project else {
        return empty("gemini", "Gemini Usage", "no_quota");
    };
    let quota = match http_post_json(
        "https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota",
        &headers,
        &json!({ "project": project }),
    ) {
        Ok((200, json)) => json,
        Ok((code, _)) => return empty("gemini", "Gemini Usage", &format!("api_error:{code}")),
        Err(e) => return empty("gemini", "Gemini Usage", &e),
    };
    let mut metrics = vec![];
    if let Some(buckets) = quota.get("buckets").and_then(|v| v.as_array()) {
        for bucket in buckets {
            let Some(remaining) = json_f64(bucket.get("remainingFraction")) else {
                continue;
            };
            let percent = ((1.0 - remaining) * 100.0).clamp(0.0, 100.0);
            let model = bucket
                .get("modelId")
                .or_else(|| bucket.get("model_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("Model");
            metrics.push(UsageMetric {
                id: model.into(),
                label: model.to_string(),
                percent,
                resets_at: bucket.get("resetTime").and_then(parse_date),
            });
        }
    }
    metrics.sort_by(|a, b| b.percent.partial_cmp(&a.percent).unwrap_or(std::cmp::Ordering::Equal));
    ProviderSnapshot {
        id: "gemini".into(),
        title: "Gemini Usage".into(),
        headline_percent: metrics.first().map(|m| m.percent),
        error: if metrics.is_empty() {
            Some("no_quota".into())
        } else {
            None
        },
        metrics,
        updated_at: now_ms(),
        reset_notice: None,
    }
}

fn antigravity_access_token() -> Option<String> {
    let paths = [
        home().join(".gemini/antigravity-cli/antigravity-oauth-token"),
        home().join(".config/antigravity-cli/antigravity-oauth-token"),
    ];
    for path in paths {
        let Ok(data) = fs::read_to_string(&path) else { continue };
        let Ok(obj) = serde_json::from_str::<Value>(&data) else { continue };
        let tok = obj.get("token").unwrap_or(&obj);
        let access = tok
            .get("access_token")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let refresh = tok
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        if !access.is_empty() {
            return Some(access);
        }
        if let Some(refresh) = refresh {
            if let Some(token) = refresh_google(&refresh, &antigravity_oauth_client(), &antigravity_oauth_secret()) {
                return Some(token);
            }
        }
    }
    env_or_shell("ANTIGRAVITY_ACCESS_TOKEN").or_else(|| {
        env_or_shell("ANTIGRAVITY_REFRESH_TOKEN")
            .and_then(|r| refresh_google(&r, &antigravity_oauth_client(), &antigravity_oauth_secret()))
    })
}

fn fetch_antigravity() -> ProviderSnapshot {
    let Some(token) = antigravity_access_token() else {
        return empty("antigravity", "Antigravity Usage", "login_not_found");
    };
    let headers = [
        ("Authorization", format!("Bearer {token}")),
        ("Content-Type", "application/json".into()),
        ("User-Agent", "antigravity/darwin/arm64".into()),
    ];
    let bases = [
        "https://cloudcode-pa.googleapis.com",
        "https://daily-cloudcode-pa.sandbox.googleapis.com",
    ];
    for base in bases {
        let load = match http_post_json(
            &format!("{base}/v1internal:loadCodeAssist"),
            &headers,
            &json!({ "metadata": { "ideType": "ANTIGRAVITY" } }),
        ) {
            Ok((200, json)) => json,
            _ => continue,
        };
        let project = load
            .get("cloudaicompanionProject")
            .and_then(|v| v.as_str())
            .or_else(|| load.pointer("/cloudaicompanionProject/id").and_then(|v| v.as_str()));
        let Some(project) = project else { continue };
        let models_json = match http_post_json(
            &format!("{base}/v1internal:fetchAvailableModels"),
            &headers,
            &json!({ "project": project }),
        ) {
            Ok((200, json)) => json,
            _ => continue,
        };
        let mut metrics = vec![];
        let models = models_json.get("models");
        if let Some(map) = models.and_then(|v| v.as_object()) {
            for (name, model) in map {
                push_antigravity_metric(&mut metrics, name, model);
            }
        } else if let Some(arr) = models.and_then(|v| v.as_array()) {
            for model in arr {
                let name = model
                    .get("name")
                    .or_else(|| model.get("id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Model");
                push_antigravity_metric(&mut metrics, name, model);
            }
        }
        metrics.sort_by(|a, b| b.percent.partial_cmp(&a.percent).unwrap_or(std::cmp::Ordering::Equal));
        if !metrics.is_empty() {
            return ProviderSnapshot {
                id: "antigravity".into(),
                title: "Antigravity Usage".into(),
                headline_percent: metrics.first().map(|m| m.percent),
                error: None,
                metrics,
                updated_at: now_ms(),
                reset_notice: None,
            };
        }
    }
    empty("antigravity", "Antigravity Usage", "no_quota")
}

fn push_antigravity_metric(metrics: &mut Vec<UsageMetric>, name: &str, model: &Value) {
    let quota = model.get("quotaInfo").unwrap_or(model);
    let Some(remaining) = json_f64(quota.get("remainingFraction")) else {
        return;
    };
    let percent = ((1.0 - remaining) * 100.0).clamp(0.0, 100.0);
    metrics.push(UsageMetric {
        id: name.into(),
        label: name.to_string(),
        percent,
        resets_at: quota.get("resetTime").and_then(parse_date),
    });
}

pub fn format_reset(ms: Option<i64>) -> Option<String> {
    format_reset_in(ms, false)
}

pub fn format_reset_in(ms: Option<i64>, zh: bool) -> Option<String> {
    format_reset_offset(ms, zh, *Local::now().offset())
}

fn format_reset_offset(ms: Option<i64>, zh: bool, offset: FixedOffset) -> Option<String> {
    let ms = ms?;
    let dt = Utc.timestamp_millis_opt(ms).single()?.with_timezone(&offset);
    let now = Utc::now().with_timezone(&offset);
    let time = dt.format("%H:%M");
    if zh {
        if dt.year() != now.year() {
            Some(format!(
                "重置 {}年{}月{}日 {time}",
                dt.year(),
                dt.month(),
                dt.day()
            ))
        } else {
            Some(format!("重置 {}月{}日 {time}", dt.month(), dt.day()))
        }
    } else if dt.year() != now.year() {
        Some(format!(
            "Resets {} {}, {}, {time}",
            dt.format("%b"),
            dt.day(),
            dt.year()
        ))
    } else {
        Some(format!("Resets {} {}, {time}", dt.format("%b"), dt.day()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jwt_sub_reads_auth_id() {
        let payload = "eyJzdWIiOiJhdXRoMHx1c2VyXzEifQ";
        let token = format!("aaa.{payload}.sig");
        assert_eq!(jwt_sub(&token).as_deref(), Some("auth0|user_1"));
    }

    #[test]
    fn cookie_encodes_pipe_in_auth_id() {
        let cookie = cursor_session_cookie("google-oauth2|user_x", "tok");
        assert_eq!(
            cookie,
            "WorkosCursorSessionToken=google-oauth2%7Cuser_x%3A%3Atok"
        );
    }

    #[test]
    fn cursor_token_keeps_long_jwt() {
        let token = "a".repeat(424);
        assert_eq!(clean_cursor_token(token.clone()).as_deref(), Some(token.as_str()));
        assert!(clean_cursor_id(token).is_none());
    }

    #[test]
    fn grok_unified_billing_without_percent_is_empty() {
        let json = json!({
            "config": {
                "currentPeriod": { "end": "2026-09-06T13:53:15.842986+00:00" },
                "isUnifiedBillingUser": true,
                "onDemandCap": { "val": 0 },
                "onDemandUsed": { "val": 0 }
            }
        });
        let snap = parse_grok_bodies(vec![json]);
        assert_eq!(snap.headline_percent, None);
        assert_eq!(snap.error.as_deref(), Some("no_quota"));
        assert!(snap.metrics.is_empty());
    }

    #[test]
    fn grok_shows_weekly_monthly_and_ondemand() {
        let json = json!({
            "config": {
                "currentPeriod": { "end": "2026-09-06T13:53:15.842986+00:00" },
                "creditUsagePercent": 12.0,
                "used": { "val": 20 },
                "monthlyLimit": { "val": 100 },
                "onDemandCap": { "val": 50 },
                "onDemandUsed": { "val": 10 }
            }
        });
        let snap = parse_grok_bodies(vec![json]);
        assert_eq!(snap.headline_percent, Some(12.0));
        assert_eq!(snap.metrics.len(), 3);
        assert!(snap.metrics.iter().any(|m| m.id == "weekly" && m.percent == 12.0));
        assert!(snap.metrics.iter().any(|m| m.id == "monthly" && m.percent == 20.0));
        assert!(snap.metrics.iter().any(|m| m.id == "ondemand" && m.percent == 20.0));
    }

    #[test]
    fn zcode_prefers_enabled_bigmodel_coding_plan() {
        let obj = json!({
            "provider": {
                "builtin:zai-coding-plan": {
                    "enabled": false,
                    "options": { "apiKey": "zzz.should-not-win" }
                },
                "builtin:bigmodel-coding-plan": {
                    "enabled": true,
                    "options": {
                        "apiKey": "abc.coding-plan",
                        "baseURL": "https://open.bigmodel.cn/api/anthropic"
                    }
                }
            }
        });
        assert_eq!(
            zcode_coding_plan_from(&obj),
            Some(("abc.coding-plan".into(), "https://open.bigmodel.cn"))
        );
    }

    #[test]
    fn zcode_skips_disabled_and_empty_keys() {
        let obj = json!({
            "provider": {
                "builtin:bigmodel-coding-plan": {
                    "enabled": true,
                    "options": { "apiKey": "" }
                },
                "builtin:zai-coding-plan": {
                    "enabled": true,
                    "options": {
                        "apiKey": "zai.plan-key",
                        "baseURL": "https://api.z.ai/api/anthropic"
                    }
                }
            }
        });
        assert_eq!(
            zcode_coding_plan_from(&obj),
            Some(("zai.plan-key".into(), "https://api.z.ai"))
        );
    }

    #[test]
    fn zhipu_quota_maps_credit_limit_weekly() {
        let snap = parse_glm(
            "zcode",
            "ZCode Usage",
            json!({
                "data": {
                    "limits": [
                        {
                            "type": "CREDIT_LIMIT",
                            "unit": 6,
                            "number": 1,
                            "percentage": 12.5,
                            "nextResetTime": 1788000000000i64
                        },
                        {
                            "type": "TOKENS_LIMIT",
                            "unit": 3,
                            "number": 5,
                            "percentage": 40.0,
                            "nextResetTime": 1788000000000i64
                        }
                    ]
                }
            }),
        );
        assert_eq!(snap.id, "zcode");
        assert_eq!(snap.headline_percent, Some(40.0));
        assert_eq!(snap.metrics[0].label, "5-hour window");
        assert_eq!(snap.metrics[1].label, "Weekly limit");
    }

    #[test]
    fn zhipu_weekly_from_credit_limit_without_percentage() {
        let snap = parse_glm(
            "glm",
            "GLM Usage",
            json!({
                "data": {
                    "limits": [
                        {
                            "type": "CREDIT_LIMIT",
                            "unit": 3,
                            "number": 5,
                            "usage": 12000,
                            "currentValue": 1200,
                            "percentage": 10
                        },
                        {
                            "type": "CREDIT_LIMIT",
                            "unit": 6,
                            "number": 1,
                            "usage": 60000,
                            "currentValue": 15000,
                            "nextResetTime": 1788000000000i64
                        }
                    ]
                }
            }),
        );
        assert_eq!(snap.metrics.len(), 2);
        assert_eq!(snap.metrics[0].label, "5-hour window");
        assert_eq!(snap.metrics[1].label, "Weekly limit");
        assert_eq!(snap.metrics[1].percent, 25.0);
        assert!(snap.metrics[1].resets_at.is_some());
    }

    #[test]
    fn zhipu_quota_maps_month_and_keeps_all_limits() {
        let snap = parse_glm(
            "glm",
            "GLM Usage",
            json!({
                "data": {
                    "limits": [
                        {
                            "type": "CREDIT_LIMIT",
                            "unit": 6,
                            "number": 30,
                            "percentage": 8.0,
                            "nextResetTime": 1788000000000i64
                        },
                        {
                            "type": "TOKENS_LIMIT",
                            "unit": 3,
                            "number": 4,
                            "percentage": 15.0,
                            "nextResetTime": 1788000000000i64
                        },
                        {
                            "type": "TIME_LIMIT",
                            "unit": 6,
                            "number": 1,
                            "percentage": 1.0,
                            "nextResetTime": 1788000000000i64
                        }
                    ]
                }
            }),
        );
        assert_eq!(snap.metrics.len(), 3);
        assert_eq!(snap.metrics[0].label, "4-hour window");
        assert_eq!(snap.metrics[1].label, "Monthly limit");
        assert_eq!(snap.metrics[2].label, "MCP tools");
        assert_eq!(snap.headline_percent, Some(8.0));
    }

    #[test]
    fn parse_date_rfc3339_and_unix() {
        let expect = DateTime::parse_from_rfc3339("2026-09-02T07:30:00Z")
            .unwrap()
            .timestamp_millis();
        assert_eq!(parse_date(&json!("2026-09-02T07:30:00Z")), Some(expect));
        assert_eq!(parse_date(&json!(expect / 1000)), Some(expect));
        assert_eq!(parse_date(&json!(expect)), Some(expect));
    }

    #[test]
    fn parse_date_naive_utc_vs_beijing() {
        let utc = parse_date(&json!("2026-09-02 15:30:00")).unwrap();
        let beijing = parse_date_beijing(&json!("2026-09-02 15:30:00")).unwrap();
        assert_eq!(utc - beijing, 8 * 3600 * 1000);
        let date_only_utc = parse_date(&json!("2026-09-02")).unwrap();
        assert_eq!(
            date_only_utc,
            DateTime::parse_from_rfc3339("2026-09-02T00:00:00Z")
                .unwrap()
                .timestamp_millis()
        );
    }

    #[test]
    fn format_reset_follows_given_offset() {
        let ms = DateTime::parse_from_rfc3339("2026-09-02T07:30:00Z")
            .unwrap()
            .timestamp_millis();
        let shanghai = beijing();
        assert_eq!(
            format_reset_offset(Some(ms), true, shanghai).as_deref(),
            Some("重置 9月2日 15:30")
        );
        assert_eq!(
            format_reset_offset(Some(ms), false, shanghai).as_deref(),
            Some("Resets Sep 2, 15:30")
        );
        let utc = FixedOffset::east_opt(0).unwrap();
        assert_eq!(
            format_reset_offset(Some(ms), false, utc).as_deref(),
            Some("Resets Sep 2, 07:30")
        );
        let next_year = shanghai
            .with_ymd_and_hms(2027, 1, 1, 0, 0, 0)
            .single()
            .unwrap()
            .timestamp_millis();
        assert_eq!(
            format_reset_offset(Some(next_year), true, shanghai).as_deref(),
            Some("重置 2027年1月1日 00:00")
        );
    }

    #[test]
    fn codex_keeps_zero_percent_extra_windows() {
        let snap = parse_codex(json!({
            "rate_limit": {
                "primary_window": {
                    "used_percent": 12.0,
                    "limit_window_seconds": 18000,
                    "reset_at": 1788000000
                },
                "secondary_window": {
                    "used_percent": 40.0,
                    "limit_window_seconds": 604800,
                    "reset_at": 1788000000
                }
            },
            "additional_rate_limits": [{
                "limit_name": "Extra",
                "rate_limit": {
                    "primary_window": {
                        "used_percent": 0.0,
                        "limit_window_seconds": 18000,
                        "reset_at": 1788000000
                    }
                }
            }]
        }));
        assert_eq!(snap.metrics.len(), 3);
        assert_eq!(snap.metrics[0].label, "5-hour window");
        assert_eq!(snap.metrics[1].label, "Weekly limit");
        assert_eq!(snap.metrics[2].label, "Extra · 5-hour window");
        assert_eq!(snap.metrics[2].percent, 0.0);
        assert_eq!(snap.headline_percent, Some(12.0));
    }

    #[test]
    fn cursor_shows_ondemand_when_enabled() {
        let snap = parse_cursor(json!({
            "billingCycleEnd": "2026-10-01T00:00:00.000Z",
            "individualUsage": {
                "plan": { "autoPercentUsed": 23.0, "apiPercentUsed": 5.0 },
                "onDemand": { "enabled": true, "used": 10, "limit": 50 }
            }
        }));
        assert_eq!(snap.metrics.len(), 3);
        assert_eq!(snap.metrics[0].label, "Included usage");
        assert_eq!(snap.metrics[2].label, "On-demand");
        assert_eq!(snap.metrics[2].percent, 20.0);
        assert!(snap.metrics[0].resets_at.is_some());
    }

    #[test]
    fn cursor_shows_grok_bot_weekly_when_included() {
        let snap = parse_cursor(json!({
            "billingCycleEnd": "2026-10-01T00:00:00.000Z",
            "individualUsage": {
                "plan": { "autoPercentUsed": 10.0, "apiPercentUsed": 0.0 }
            },
            "grokBot": {
                "hasNonZeroIncludedLimit": true,
                "usagePercent": 37.5,
                "nextResetTimestampUtc": "2026-09-09T00:00:00.000Z"
            }
        }));
        let bot = snap.metrics.iter().find(|m| m.id == "grok-bot").unwrap();
        assert_eq!(bot.label, "Grok Bot · Weekly limit");
        assert_eq!(bot.percent, 37.5);
        assert!(bot.resets_at.is_some());
    }

    #[test]
    fn cursor_hides_grok_bot_without_allowance() {
        assert!(parse_cursor_grok_bot(&json!({
            "hasNonZeroIncludedLimit": false,
            "usagePercent": 0.0
        }))
        .is_none());
        assert!(parse_cursor_grok_bot(&json!({
            "usagePercent": 12.0
        }))
        .is_none());
    }

    #[test]
    fn claude_shows_scoped_weekly_and_extra_usage() {
        let snap = parse_claude(json!({
            "limits": [
                { "kind": "session", "percent": 11.0, "resets_at": "2026-09-02T10:00:00Z" },
                { "kind": "weekly_all", "percent": 22.0, "resets_at": "2026-09-06T00:00:00Z" },
                {
                    "kind": "weekly_scoped",
                    "percent": 33.0,
                    "scope": { "model": { "display_name": "Fable" } },
                    "resets_at": "2026-09-06T00:00:00Z"
                }
            ],
            "extra_usage": {
                "is_enabled": true,
                "monthly_limit": 100000,
                "used_credits": 45710,
                "utilization": 45.71
            }
        }));
        assert_eq!(snap.headline_percent, Some(11.0));
        assert_eq!(snap.metrics.len(), 4);
        assert_eq!(snap.metrics[2].label, "Fable · Weekly limit");
        assert_eq!(snap.metrics[3].label, "Monthly limit");
        assert!((snap.metrics[3].percent - 45.71).abs() < 0.001);
    }

    #[test]
    fn copilot_lists_capped_snapshots_only() {
        let snap = parse_copilot(json!({
            "quota_reset_date_utc": "2026-10-01T00:00:00.000Z",
            "quota_snapshots": {
                "chat": { "unlimited": true, "entitlement": 0, "remaining": 0 },
                "premium_interactions": {
                    "unlimited": false,
                    "entitlement": 100,
                    "remaining": 40,
                    "percent_remaining": 40.0
                },
                "completions": {
                    "unlimited": false,
                    "entitlement": 50,
                    "remaining": 25,
                    "percent_remaining": 50.0
                }
            }
        }));
        assert_eq!(snap.metrics.len(), 2);
        assert_eq!(snap.metrics[0].label, "Premium requests");
        assert_eq!(snap.metrics[0].percent, 60.0);
        assert_eq!(snap.metrics[1].label, "Completions");
        assert_eq!(snap.metrics[1].percent, 50.0);
        assert!(snap.metrics[0].resets_at.is_some());
    }
}
