use serde::Serialize;
use serde_json::Value;

const RELEASES_API: &str = "https://api.github.com/repos/rayelzz/UsageBar-tauri/releases/latest";
pub const RELEASES_PAGE: &str = "https://github.com/rayelzz/UsageBar-tauri/releases/latest";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub url: String,
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
    if !is_newer(tag, current) {
        return None;
    }
    let latest = tag.trim().trim_start_matches('v').to_string();
    let url = json
        .get("html_url")
        .and_then(|v| v.as_str())
        .unwrap_or(RELEASES_PAGE)
        .to_string();
    Some(UpdateInfo {
        current: current.into(),
        latest,
        url,
    })
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
}
