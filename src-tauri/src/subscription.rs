use std::time::Duration;

use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

const MAX_SUBSCRIPTION_BYTES: usize = 8 * 1024 * 1024;

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
    download_with_client(&client, url).await
}

async fn download_with_client(client: &reqwest::Client, url: &str) -> Result<Downloaded> {
    let mut resp = client.get(url).send().await.map_err(|e| Error::Http(e.without_url()))?
        .error_for_status().map_err(|e| Error::Http(e.without_url()))?;
    let headers = resp.headers().clone();
    let too_large = || Error::msg("Subscription download exceeds the 8 MiB limit.");
    if resp.content_length().is_some_and(|size| size > MAX_SUBSCRIPTION_BYTES as u64) {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| Error::Http(e.without_url()))? {
        if chunk.len() > MAX_SUBSCRIPTION_BYTES - bytes.len() {
            return Err(too_large());
        }
        bytes.extend_from_slice(&chunk);
    }
    let content = String::from_utf8(bytes)
        .unwrap_or_else(|err| String::from_utf8_lossy(err.as_bytes()).into_owned());

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
    fn rejects_oversized_declared_and_chunked_subscriptions() {
        use std::io::{BufRead, BufReader, ErrorKind, Write};
        use std::net::TcpListener;
        use std::time::Instant;

        let timeout = Duration::from_secs(5);
        let client = reqwest::Client::builder().no_proxy().timeout(timeout).build().unwrap();

        for chunked in [false, true] {
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            listener.set_nonblocking(true).unwrap();
            let url = format!("http://{}/?token=private", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let deadline = Instant::now() + timeout;
                let mut socket = loop {
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(error) if error.kind() == ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "No connection to the subscription fixture");
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(error) => panic!("Could not accept fixture connection: {error}"),
                    }
                };
                socket.set_nonblocking(false).unwrap();
                socket.set_read_timeout(Some(timeout)).unwrap();
                socket.set_write_timeout(Some(timeout)).unwrap();
                let mut request = BufReader::new(&mut socket);
                let mut line = String::new();
                while request.read_line(&mut line).unwrap() != 0 && line != "\r\n" {
                    line.clear();
                }
                if chunked {
                    socket.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n").unwrap();
                    let body = vec![b'a'; MAX_SUBSCRIPTION_BYTES + 1];
                    write!(socket, "{:x}\r\n", body.len()).unwrap();
                    socket.write_all(&body).unwrap();
                    let _ = socket.write_all(b"\r\n0\r\n\r\n");
                } else {
                    write!(socket, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", MAX_SUBSCRIPTION_BYTES + 1).unwrap();
                }
            });
            let error = tauri::async_runtime::block_on(download_with_client(&client, &url)).err().unwrap();
            server.join().unwrap();
            assert_eq!(error.to_string(), "Subscription download exceeds the 8 MiB limit.");
        }
    }

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
