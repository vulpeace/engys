use std::time::Duration;

use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Parsed `subscription-userinfo` header: traffic quota and expiry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionInfo {
    pub upload: u64,
    pub download: u64,
    pub total: u64,
    /// Unix seconds; 0/None = never expires.
    pub expire: Option<u64>,
}

#[derive(Serialize, Deserialize)]
pub struct Downloaded {
    pub content: String,
    pub info: Option<SubscriptionInfo>,
    pub suggested_name: Option<String>,
    pub update_interval_hours: Option<u32>,
}

pub async fn download(url: &str, user_agent: &str) -> Result<Downloaded> {
    let client = reqwest::Client::builder()
        .user_agent(user_agent)
        .timeout(Duration::from_secs(30))
        .build()?;
    let resp = client.get(url).send().await.map_err(|e| Error::Http(e.without_url()))?
        .error_for_status().map_err(|e| Error::Http(e.without_url()))?;
    let headers = resp.headers().clone();
    let content = resp.text().await.map_err(|e| Error::Http(e.without_url()))?;

    let info = headers
        .get("subscription-userinfo")
        .and_then(|v| v.to_str().ok())
        .map(parse_userinfo);
    let suggested_name = headers
        .get("content-disposition")
        .and_then(|v| v.to_str().ok())
        .and_then(parse_filename);
    let update_interval_hours = headers
        .get("profile-update-interval")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse().ok());

    validate_profile_yaml(&content).map_err(|e| {
        if looks_like_base64(&content) {
            Error::msg(
                "The subscription returned a non-Clash format (looks like a base64 node list). \
                 Use a Clash/mihomo-compatible subscription link.",
            )
        } else {
            e
        }
    })?;

    Ok(Downloaded { content, info, suggested_name, update_interval_hours })
}

/// Profiles must be a YAML mapping at the top level.
pub fn validate_profile_yaml(content: &str) -> Result<()> {
    let v: serde_yaml::Value = serde_yaml::from_str(content)?;
    if !v.is_mapping() {
        return Err(Error::msg("Profile is not a YAML mapping (expected keys like `proxies`, `rules`)."));
    }
    Ok(())
}

fn parse_userinfo(s: &str) -> SubscriptionInfo {
    let mut info = SubscriptionInfo::default();
    for part in s.split(';') {
        let mut kv = part.splitn(2, '=');
        let (Some(k), Some(v)) = (kv.next(), kv.next()) else { continue };
        let v: u64 = v.trim().parse::<f64>().map(|f| f as u64).unwrap_or(0);
        match k.trim() {
            "upload" => info.upload = v,
            "download" => info.download = v,
            "total" => info.total = v,
            "expire" => info.expire = (v > 0).then_some(v),
            _ => {}
        }
    }
    info
}

fn parse_filename(cd: &str) -> Option<String> {
    // Prefer RFC 5987 `filename*=UTF-8''...`, fall back to `filename="..."`.
    for part in cd.split(';').map(str::trim) {
        if let Some(v) = part.strip_prefix("filename*=") {
            let v = v.split("''").nth(1).unwrap_or(v);
            return Some(percent_decode_str(v).decode_utf8_lossy().into_owned());
        }
    }
    for part in cd.split(';').map(str::trim) {
        if let Some(v) = part.strip_prefix("filename=") {
            return Some(v.trim_matches('"').to_string());
        }
    }
    None
}

fn looks_like_base64(s: &str) -> bool {
    let t = s.trim();
    !t.is_empty()
        && !t.contains(':')
        && t.chars().all(|c| c.is_ascii_alphanumeric() || "+/=\r\n-_".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn userinfo() {
        let i = parse_userinfo("upload=10; download=2048; total=1073741824; expire=1700000000");
        assert_eq!(i.upload, 10);
        assert_eq!(i.download, 2048);
        assert_eq!(i.total, 1073741824);
        assert_eq!(i.expire, Some(1700000000));
    }

    #[test]
    fn filename() {
        assert_eq!(parse_filename("attachment; filename*=UTF-8''My%20Sub").as_deref(), Some("My Sub"));
        assert_eq!(parse_filename("attachment; filename=\"a.yaml\"").as_deref(), Some("a.yaml"));
    }
}
