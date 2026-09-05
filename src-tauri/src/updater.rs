use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

const RELEASES_API: &str = "https://api.github.com/repos/rayelzz/UsageBar-tauri/releases/latest";
pub const RELEASES_PAGE: &str = "https://github.com/rayelzz/UsageBar-tauri/releases/latest";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub url: String,
    pub has_update: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes_en: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes_zh: Option<String>,
}

fn norm_heading(line: &str) -> String {
    line.trim()
        .trim_start_matches('#')
        .trim()
        .replace('\u{2019}', "'")
        .replace('\u{2018}', "'")
        .to_ascii_lowercase()
}

fn is_notes_start(line: &str, lang: &str) -> bool {
    let h = norm_heading(line);
    match lang {
        "en" => h == "what's new" || h == "whats new",
        "zh" => h == "更新说明",
        _ => false,
    }
}

fn is_notes_stop(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed == "---" {
        return true;
    }
    let h = norm_heading(line);
    matches!(
        h.as_str(),
        "download" | "downloads" | "下载" | "what's new" | "whats new" | "更新说明"
    )
}

fn extract_notes_section(body: &str, lang: &str) -> Option<String> {
    let mut lines = body.lines().peekable();
    while let Some(line) = lines.next() {
        if !is_notes_start(line, lang) {
            continue;
        }
        let mut out = vec![line.trim().to_string()];
        for next in lines.by_ref() {
            if is_notes_stop(next) && !is_notes_start(next, lang) {
                break;
            }
            out.push(next.trim_end().to_string());
        }
        let text = out
            .join("\n")
            .trim()
            .trim_matches('\n')
            .to_string();
        if text.is_empty() {
            return None;
        }
        return Some(text);
    }
    None
}

pub fn parse_release_notes(body: &str) -> (Option<String>, Option<String>) {
    (
        extract_notes_section(body, "en"),
        extract_notes_section(body, "zh"),
    )
}

pub fn current_version() -> String {
    env!("CARGO_PKG_VERSION").into()
}

pub fn should_prompt(latest: &str, skipped: &str) -> bool {
    let skipped = skipped.trim().trim_start_matches('v');
    if skipped.is_empty() {
        return true;
    }
    is_newer(latest, skipped)
}

fn parse_ver(raw: &str) -> Option<(u64, u64, u64)> {
    let s = raw.trim().trim_start_matches('v');
    let mut parts = s.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts
        .next()
        .unwrap_or("0")
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .unwrap_or("0")
        .parse()
        .ok()?;
    Some((major, minor, patch))
}

fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_ver(latest), parse_ver(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

pub fn check() -> Option<UpdateInfo> {
    let current = env!("CARGO_PKG_VERSION");
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent(format!("UsageBar/{current}"))
        .build()
        .ok()?;
    let resp = client
        .get(RELEASES_API)
        .header("Accept", "application/vnd.github+json")
        .send()
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let json: Value = resp.json().ok()?;
    let tag = json.get("tag_name")?.as_str()?;
    let latest = tag.trim().trim_start_matches('v').to_string();
    let url = json
        .get("html_url")
        .and_then(|v| v.as_str())
        .unwrap_or(RELEASES_PAGE)
        .to_string();
    let body = json.get("body").and_then(|v| v.as_str()).unwrap_or("");
    let (notes_en, notes_zh) = parse_release_notes(body);
    Some(UpdateInfo {
        current: current.into(),
        latest: latest.clone(),
        url,
        has_update: is_newer(&latest, current),
        notes_en,
        notes_zh,
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    pub phase: String,
    pub downloaded: u64,
    pub total: Option<u64>,
}

fn latest_json_url() -> Option<String> {
    let current = env!("CARGO_PKG_VERSION");
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent(format!("UsageBar/{current}"))
        .build()
        .ok()?;
    let json: Value = client
        .get(RELEASES_API)
        .header("Accept", "application/vnd.github+json")
        .send()
        .ok()?
        .json()
        .ok()?;
    json.get("assets")
        .and_then(|a| a.as_array())
        .and_then(|arr| {
            arr.iter().find_map(|asset| {
                let name = asset.get("name")?.as_str()?;
                if name == "latest.json" {
                    asset
                        .get("browser_download_url")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                } else {
                    None
                }
            })
        })
}

pub async fn install(app: AppHandle) -> Result<(), String> {
    let emit = |phase: &str, downloaded: u64, total: Option<u64>| {
        let _ = app.emit(
            "usagebar-update-progress",
            UpdateProgress {
                phase: phase.into(),
                downloaded,
                total,
            },
        );
    };
    emit("check", 0, None);
    let update = match app.updater().map_err(|e| e.to_string())?.check().await {
        Ok(Some(update)) => update,
        Ok(None) => return Err("no update".into()),
        Err(_) => {
            let url = tauri::async_runtime::spawn_blocking(latest_json_url)
                .await
                .ok()
                .flatten()
                .ok_or_else(|| "no update".to_string())?;
            let endpoint = url.parse().map_err(|e| format!("{e}"))?;
            app.updater_builder()
                .endpoints(vec![endpoint])
                .map_err(|e| e.to_string())?
                .build()
                .map_err(|e| e.to_string())?
                .check()
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "no update".to_string())?
        }
    };
    let mut downloaded = 0u64;
    update
        .download_and_install(
            |chunk, total| {
                downloaded += chunk as u64;
                emit("download", downloaded, total);
            },
            || emit("finish", 0, None),
        )
        .await
        .map_err(|e| e.to_string())?;
    emit("restart", 0, None);
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_tag_wins() {
        assert!(is_newer("v0.0.9", "0.0.8"));
        assert!(is_newer("0.1.0", "0.0.9"));
        assert!(!is_newer("v0.0.8", "0.0.8"));
        assert!(!is_newer("0.0.7", "0.0.8"));
        assert!(!is_newer("bad", "0.0.8"));
    }

    #[test]
    fn skipped_version_waits_for_next() {
        assert!(should_prompt("0.0.11", ""));
        assert!(should_prompt("v0.0.11", "  "));
        assert!(!should_prompt("0.0.10", "0.0.10"));
        assert!(!should_prompt("v0.0.10", "0.0.10"));
        assert!(should_prompt("0.0.11", "0.0.10"));
        assert!(!should_prompt("0.0.9", "0.0.10"));
    }

    #[test]
    fn parses_bilingual_release_notes_and_drops_download() {
        let body = r#"### What’s new

- **Reset cards:** GLM and ZCode read the signed-in ZCode session.
- Prepaid / Extra Usage is still not a reset card.

See [CHANGELOG.md](https://example.com) for the full history.

### Download

- **macOS:** `aarch64.dmg`

---

### 更新说明

- **重置卡：** GLM / ZCode 读本机已登录的 ZCode 会话。
- 预付余额 / Extra Usage 仍然不是重置卡。

完整记录见 [CHANGELOG.md](https://example.com)。

### 下载

- **macOS：** `aarch64.dmg`
"#;
        let (en, zh) = parse_release_notes(body);
        let en = en.expect("en notes");
        let zh = zh.expect("zh notes");
        assert!(en.contains("What’s new") || en.contains("What's new"));
        assert!(en.contains("Reset cards"));
        assert!(en.contains("CHANGELOG.md"));
        assert!(!en.contains("aarch64.dmg"));
        assert!(!en.contains("更新说明"));
        assert!(zh.contains("更新说明"));
        assert!(zh.contains("重置卡"));
        assert!(zh.contains("CHANGELOG.md"));
        assert!(!zh.contains("aarch64.dmg"));
        assert!(!zh.contains("What’s new") && !zh.contains("What's new"));
    }

    #[test]
    fn missing_section_is_none() {
        let (en, zh) = parse_release_notes("### Download\n\n- dmg\n");
        assert!(en.is_none());
        assert!(zh.is_none());
    }
}
