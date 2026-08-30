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
pub struct ProviderSnapshot {
    pub id: String,
    pub title: String,
    pub headline_percent: Option<f64>,
    pub metrics: Vec<UsageMetric>,
    pub error: Option<String>,
    pub updated_at: i64,
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
        None => return empty("codex", "Codex Usage", "未找到 Codex 登录信息"),
    };
    match pull_codex(&auth) {
        Ok(snap) => snap,
        Err(_) => {
            if refresh_codex(&mut auth, &auth_path) {
                pull_codex(&auth).unwrap_or_else(|e| empty("codex", "Codex Usage", &e))
            } else {
                empty("codex", "Codex Usage", "未找到 Codex 登录信息")
            }
        }
    }
}

fn pull_codex(auth: &Value) -> Result<ProviderSnapshot, String> {
    let tokens = auth.get("tokens").ok_or("未找到 Codex 登录信息")?;
    let access = tokens
        .get("access_token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or("未找到 Codex 登录信息")?;
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
        return Err("Codex 接口异常".into());
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
                metrics
                    .into_iter()
                    .filter(|m| m.id != primary.id && m.percent > 0.5),
            )
            .collect();
    }
    let headline = metrics.first().map(|m| m.percent);
    Ok(ProviderSnapshot {
        id: "codex".into(),
        title: "Codex Usage".into(),
        headline_percent: headline,
        error: if metrics.is_empty() {
            Some("无额度数据".into())
        } else {
            None
        },
        metrics,
        updated_at: now_ms(),
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

fn cursor_db() -> PathBuf {
    if cfg!(target_os = "macos") {
        home().join("Library/Application Support/Cursor/User/globalStorage/state.vscdb")
    } else if cfg!(target_os = "windows") {
        dirs::data_dir()
            .unwrap_or_else(|| home().join("AppData/Roaming"))
            .join("Cursor/User/globalStorage/state.vscdb")
    } else {
        dirs::config_dir()
            .unwrap_or_else(|| home().join(".config"))
            .join("Cursor/User/globalStorage/state.vscdb")
    }
}

fn fetch_cursor() -> ProviderSnapshot {
    let db = cursor_db();
    let (Some(token), Some(user_id)) = (
        sqlite_value(&db, "cursorAuth/accessToken"),
        sqlite_value(&db, "cursorAuth/userId"),
    ) else {
        return empty("cursor", "Cursor Usage", "未找到 Cursor 登录信息");
    };
    let cookie = format!("WorkosCursorSessionToken={user_id}%3A%3A{token}");
    let headers = [
        ("Accept", "application/json".into()),
        ("User-Agent", "Mozilla/5.0 UsageBar/1.0".into()),
        ("Origin", "https://cursor.com".into()),
        ("Referer", "https://cursor.com/dashboard?tab=usage".into()),
        ("Cookie", cookie),
    ];
    let json = match http_get_json("https://cursor.com/api/usage-summary", &headers) {
        Ok((200, json)) => json,
        Ok((code, _)) => {
            return empty(
                "cursor",
                "Cursor Usage",
                &format!("Cursor 接口异常 ({code})"),
            )
        }
        Err(e) => return empty("cursor", "Cursor Usage", &e),
    };
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
            Some("无额度数据".into())
        } else {
            None
        },
        metrics,
        updated_at: now_ms(),
    }
}

// MARK: Grok

fn fetch_grok() -> ProviderSnapshot {
    let Some((token, refresh, user_id, issuer, client_id)) = load_grok() else {
        return empty("grok", "Grok Usage", "未找到 Grok 登录信息");
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
    let url = "https://cli-chat-proxy.grok.com/v1/billing?format=credits";
    let mut result = http_get_json(url, &headers);
    if let Ok((401, _)) = &result {
        if let Some(new_token) = refresh_grok(issuer.as_deref(), refresh.as_deref(), client_id.as_deref())
        {
            headers[0] = ("Authorization", format!("Bearer {new_token}"));
            result = http_get_json(url, &headers);
        }
    }
    let Ok((200, json)) = result else {
        return empty("grok", "Grok Usage", "Grok 接口异常");
    };
    let config = json.get("config").cloned().unwrap_or(json.clone());
    let reset = config
        .pointer("/currentPeriod/end")
        .and_then(parse_date)
        .or_else(|| config.get("billingPeriodEnd").and_then(parse_date));
    let weekly = config.get("creditUsagePercent").and_then(|v| v.as_f64());
    let mut metrics = vec![];
    if let Some(weekly) = weekly {
        metrics.push(UsageMetric {
            id: "weekly".into(),
            label: "Weekly allowance".into(),
            percent: weekly,
            resets_at: reset,
        });
    }
    if let Some(products) = config.get("productUsage").and_then(|v| v.as_array()) {
        for product in products {
            let name = product
                .get("product")
                .and_then(|v| v.as_str())
                .unwrap_or("Product");
            let Some(pct) = product.get("usagePercent").and_then(|v| v.as_f64()) else {
                continue;
            };
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
    ProviderSnapshot {
        id: "grok".into(),
        title: "Grok Usage".into(),
        headline_percent: weekly.or_else(|| metrics.first().map(|m| m.percent)),
        error: if metrics.is_empty() {
            Some("无额度数据".into())
        } else {
            None
        },
        metrics,
        updated_at: now_ms(),
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
        return empty("glm", "GLM Usage", "未找到 GLM 登录信息");
    };
    let headers = [
        ("Authorization", format!("Bearer {key}")),
        ("Accept", "application/json".into()),
    ];
    for endpoint in [
        "https://api.z.ai/api/monitor/usage/quota/limit",
        "https://open.bigmodel.cn/api/monitor/usage/quota/limit",
    ] {
        if let Ok((200, json)) = http_get_json(endpoint, &headers) {
            return parse_glm(json);
        }
    }
    empty("glm", "GLM Usage", "未找到 GLM 登录信息")
}

fn parse_glm(json: Value) -> ProviderSnapshot {
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
        let label = if typ == "TOKENS_LIMIT" && unit == 3 && number == 5 {
            "5-hour window"
        } else if typ == "TOKENS_LIMIT" && unit == 6 && number == 7 {
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
        id: "glm".into(),
        title: "GLM Usage".into(),
        headline_percent: headline,
        error: if metrics.is_empty() {
            Some("无额度数据".into())
        } else {
            None
        },
        metrics,
        updated_at: now_ms(),
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
    cc_switch_glm_key()
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

pub fn fetch_all() -> Vec<ProviderSnapshot> {
    vec![fetch_codex(), fetch_cursor(), fetch_grok(), fetch_glm()]
}

pub fn format_reset(ms: Option<i64>) -> Option<String> {
    let ms = ms?;
    let dt = Utc.timestamp_millis_opt(ms).single()?;
    Some(format!(
        "Resets {}",
        dt.with_timezone(&Local).format("%a %I:%M %p")
    ))
}
