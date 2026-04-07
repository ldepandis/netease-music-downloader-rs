use anyhow::Result;
use once_cell::sync::Lazy;
use reqwest::Client;
use serde_json::Value;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct ProxyEntry {
    pub host: String,
    pub port: u16,
    pub protocol: String,
}

impl ProxyEntry {
    pub fn url(&self) -> String {
        format!("{}://{}:{}", self.protocol, self.host, self.port)
    }
}

#[derive(Default)]
struct ProxyCache {
    list: Vec<ProxyEntry>,
    last_update: Option<Instant>,
}

static CACHE: Lazy<Mutex<ProxyCache>> = Lazy::new(|| Mutex::new(ProxyCache::default()));
const UPDATE_INTERVAL: Duration = Duration::from_secs(10 * 60);
const TIMEOUT: Duration = Duration::from_secs(3);

fn parse_geonode_json(v: &Value) -> Vec<ProxyEntry> {
    let mut out = Vec::new();
    if let Some(items) = v.get("data").and_then(|x| x.as_array()) {
        for item in items {
            let host = item.get("ip").and_then(|x| x.as_str()).unwrap_or_default();
            let port = item
                .get("port")
                .and_then(|x| x.as_u64())
                .or_else(|| {
                    item.get("port")
                        .and_then(|x| x.as_str())
                        .and_then(|s| s.parse::<u64>().ok())
                })
                .unwrap_or(0);
            let proto = item
                .get("protocols")
                .and_then(|x| x.as_array())
                .and_then(|arr| arr.first())
                .and_then(|x| x.as_str())
                .unwrap_or("http");
            if !host.is_empty() && port > 0 && port <= u16::MAX as u64 {
                out.push(ProxyEntry {
                    host: host.to_string(),
                    port: port as u16,
                    protocol: proto.to_string(),
                });
            }
        }
    }
    out
}

fn parse_proxyscrape_text(body: &str) -> Vec<ProxyEntry> {
    let mut out = Vec::new();
    for line in body.lines() {
        let parts: Vec<&str> = line.trim().split(':').collect();
        if parts.len() == 2
            && let Ok(port) = parts[1].parse::<u16>()
        {
            out.push(ProxyEntry {
                host: parts[0].to_string(),
                port,
                protocol: "http".to_string(),
            });
        }
    }
    out
}

fn parse_fate0_lines(body: &str) -> Vec<ProxyEntry> {
    let mut out = Vec::new();
    for line in body.lines() {
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            let country = v.get("country").and_then(|x| x.as_str()).unwrap_or_default();
            if country != "CN" {
                continue;
            }
            let host = v.get("host").and_then(|x| x.as_str()).unwrap_or_default();
            let port = v.get("port").and_then(|x| x.as_u64()).unwrap_or(0);
            let proto = v.get("type").and_then(|x| x.as_str()).unwrap_or("http");
            if !host.is_empty() && port > 0 && port <= u16::MAX as u64 {
                out.push(ProxyEntry {
                    host: host.to_string(),
                    port: port as u16,
                    protocol: proto.to_string(),
                });
            }
        }
    }
    out
}

async fn fetch_candidates(client: &Client) -> Vec<ProxyEntry> {
    let mut out = Vec::new();

    if let Ok(resp) = client
        .get("https://proxylist.geonode.com/api/proxy-list?filterUpTime=90&country=CN&protocols=http%2Chttps&limit=100")
        .send()
        .await
        && let Ok(v) = resp.json::<Value>().await
    {
        out.extend(parse_geonode_json(&v));
    }

    if let Ok(resp) = client
        .get("https://api.proxyscrape.com/v2/?request=getproxies&protocol=http&country=CN&ssl=all&anonymity=all")
        .send()
        .await
        && let Ok(body) = resp.text().await
    {
        out.extend(parse_proxyscrape_text(&body));
    }

    if let Ok(resp) = client
        .get("https://raw.githubusercontent.com/fate0/proxylist/master/proxy.list")
        .send()
        .await
        && let Ok(body) = resp.text().await
    {
        out.extend(parse_fate0_lines(&body));
    }

    out.sort_by(|a, b| a.host.cmp(&b.host).then(a.port.cmp(&b.port)));
    out.dedup_by(|a, b| a.host == b.host && a.port == b.port && a.protocol == b.protocol);
    out
}

async fn test_proxy(entry: &ProxyEntry) -> bool {
    let proxy = match reqwest::Proxy::all(entry.url()) {
        Ok(p) => p,
        Err(_) => return false,
    };

    let client = match Client::builder()
        .timeout(TIMEOUT)
        .proxy(proxy)
        .user_agent("Mozilla/5.0")
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };

    client
        .get("https://music.163.com")
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

pub async fn get_auto_proxy(force_update: bool) -> Result<Option<String>> {
    let mut needs_update = force_update;
    {
        let cache = CACHE.lock().expect("proxy cache lock");
        if cache.last_update.is_none()
            || cache
                .last_update
                .is_some_and(|t| Instant::now().duration_since(t) > UPDATE_INTERVAL)
        {
            needs_update = true;
        }
    }

    if needs_update {
        let client = Client::builder().timeout(TIMEOUT).build()?;
        let list = fetch_candidates(&client).await;
        let mut cache = CACHE.lock().expect("proxy cache lock");
        cache.list = list;
        cache.last_update = Some(Instant::now());
    }

    let candidates = {
        let cache = CACHE.lock().expect("proxy cache lock");
        cache.list.clone()
    };

    for entry in candidates.iter().take(30) {
        if test_proxy(entry).await {
            return Ok(Some(entry.url()));
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_geonode_json_extracts_valid_items() {
        let input = json!({
            "data": [
                {"ip": "1.1.1.1", "port": "8080", "protocols": ["http"]},
                {"ip": "2.2.2.2", "port": 443, "protocols": ["https"]},
                {"ip": "", "port": 9999, "protocols": ["http"]}
            ]
        });
        let out = parse_geonode_json(&input);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].url(), "http://1.1.1.1:8080");
        assert_eq!(out[1].url(), "https://2.2.2.2:443");
    }

    #[test]
    fn parse_proxyscrape_text_extracts_host_port() {
        let body = "8.8.8.8:80\ninvalid\n9.9.9.9:8080\n";
        let out = parse_proxyscrape_text(body);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].url(), "http://8.8.8.8:80");
        assert_eq!(out[1].url(), "http://9.9.9.9:8080");
    }

    #[test]
    fn parse_fate0_lines_filters_only_cn() {
        let body = r#"{"host":"10.0.0.1","port":8888,"type":"http","country":"CN"}
{"host":"10.0.0.2","port":9999,"type":"http","country":"US"}"#;
        let out = parse_fate0_lines(body);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].url(), "http://10.0.0.1:8888");
    }
}
