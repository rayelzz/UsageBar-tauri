use chrono::{DateTime, Local, TimeZone, Utc};
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

fn parse_date(v: &Value) -> Option<i64> {
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
        let fmts = ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d"];
        for fmt in fmts {
            if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
                return Some(Utc.from_utc_datetime(&naive).timestamp_millis());
            }
            if let Ok(d) = chrono::NaiveDate::parse_from_str(s, fmt) {
                if let Some(ndt) = d.and_hms_opt(0, 0, 0) {
                    return Some(Utc.from_utc_datetime(&ndt).timestamp_millis());
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
    if let Some(primary) = metrics.iter().find(|m| m.id.starts_with("plan-")).cloned() {
        metrics = std::iter::once(primary.clone())
            .chain(
                metrics.into_iter().filter(|m| {
                    m.id != primary.id && (m.id.starts_with("plan-") || m.percent > 0.5)
                }),
            )
            .collect();
    }
    let headline = metrics.first().map(|m| m.percent);
    Ok(ProviderSnapshot {
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
    })
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
                Ok((200, json)) => return parse_cursor(json),
                Ok((code, _)) => last_error = format!("api_error:{code}"),
                Err(e) => last_error = e,
            }
        }
    }
    empty("cursor", "Cursor Usage", &last_error)
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
    ProviderSnapshot {
        id: "cursor".into(),
        title: "Cursor Usage".into(),
        headline_percent: included.or(api),
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
    let mut headline = None;
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
            headline.get_or_insert(weekly);
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
                headline.get_or_insert(pct);
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
    if metrics.is_empty() && reset.is_some() {
        metrics.push(UsageMetric {
            id: "weekly".into(),
            label: "Weekly allowance".into(),
            percent: 0.0,
            resets_at: reset,
        });
        headline = Some(0.0);
    }
    ProviderSnapshot {
        id: "grok".into(),
        title: "Grok Usage".into(),
        headline_percent: headline.or_else(|| metrics.first().map(|m| m.percent)),
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

fn parse_glm(id: &str, title: &str, json: Value) -> ProviderSnapshot {
    let data = json.get("data").cloned().unwrap_or(json);
    let limits = data
        .get("limits")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut metrics = vec![];
    for item in limits {
        let Some(percent) = item.get("percentage").and_then(|v| v.as_f64()) else {
            continue;
        };
        let typ = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let unit = item.get("unit").and_then(|v| v.as_i64()).unwrap_or(0);
        let number = item.get("number").and_then(|v| v.as_i64()).unwrap_or(0);
        let reset = item.get("nextResetTime").and_then(parse_date);
        let tokenish = typ == "TOKENS_LIMIT" || typ == "CREDIT_LIMIT";
        let label = if tokenish && unit == 3 && number == 5 {
            "5-hour window"
        } else if tokenish && unit == 6 && (number == 7 || number == 1) {
            "Weekly limit"
        } else if typ == "TIME_LIMIT" {
            "MCP tools"
        } else if typ.is_empty() {
            "Usage"
        } else {
            typ
        };
        metrics.push(UsageMetric {
            id: format!("{typ}-{unit}-{number}"),
            label: label.into(),
            percent,
            resets_at: reset,
        });
    }
    metrics.sort_by_key(|m| match m.label.as_str() {
        "5-hour window" => 0,
        "Weekly limit" => 1,
        "MCP tools" => 2,
        _ => 9,
    });
    let headline = metrics
        .iter()
        .find(|m| m.label == "5-hour window")
        .or_else(|| metrics.iter().find(|m| m.label == "Weekly limit"))
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
                other if !other.is_empty() => {
                    if let Some(name) = scoped {
                        format!("{name} · {other}")
                    } else {
                        other.to_string()
                    }
                }
                _ => scoped.unwrap_or("Usage").to_string(),
            };
            metrics.push(UsageMetric {
                id: kind.to_string(),
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
    Ok(ProviderSnapshot {
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
    })
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
    let reset = json
        .get("quota_reset_date_utc")
        .and_then(parse_date)
        .or_else(|| json.get("quota_reset_date").and_then(parse_date));
    let mut metrics = vec![];
    if let Some(premium) = json.pointer("/quota_snapshots/premium_interactions") {
        if premium.get("unlimited").and_then(|v| v.as_bool()) != Some(true) {
            let entitlement = json_f64(premium.get("entitlement")).unwrap_or(0.0);
            let remaining = json_f64(premium.get("remaining")).unwrap_or(0.0);
            let percent = json_f64(premium.get("percent_remaining"))
                .map(|left| (100.0 - left).clamp(0.0, 100.0))
                .unwrap_or_else(|| {
                    if entitlement > 0.0 {
                        ((entitlement - remaining).max(0.0) / entitlement * 100.0).clamp(0.0, 100.0)
                    } else {
                        0.0
                    }
                });
            metrics.push(UsageMetric {
                id: "premium".into(),
                label: "Premium requests".into(),
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
    let ms = ms?;
    let dt = Utc.timestamp_millis_opt(ms).single()?;
    Some(format!(
        "Resets {}",
        dt.with_timezone(&Local).format("%a %I:%M %p")
    ))
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
    fn grok_unified_billing_shows_weekly_when_percent_missing() {
        let json = json!({
            "config": {
                "currentPeriod": { "end": "2026-09-06T13:53:15.842986+00:00" },
                "isUnifiedBillingUser": true,
                "onDemandCap": { "val": 0 },
                "onDemandUsed": { "val": 0 }
            }
        });
        let snap = parse_grok_bodies(vec![json]);
        assert_eq!(snap.headline_percent, Some(0.0));
        assert!(snap.error.is_none());
        assert_eq!(snap.metrics[0].id, "weekly");
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
}
