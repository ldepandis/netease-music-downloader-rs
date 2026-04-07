use crate::proxy::get_auto_proxy;
use crate::types::{Album, AlbumInfo, Artist, Availability, Song};
use anyhow::{Context, Result, anyhow};
use openssl::symm::{Cipher, encrypt};
use rand::Rng;
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue, ORIGIN, REFERER, USER_AGENT};
use reqwest::{Client, Proxy};
use serde_json::{Value, json};
use std::time::Duration;
use url::Url;

const EAPI_KEY: &str = "e82ckenh8dichen8";
const QUALITY_LEVELS: [&str; 5] = ["hires", "lossless", "exhigh", "higher", "standard"];

#[derive(Clone, Debug)]
pub struct NeteaseClient {
    pub proxy_url: Option<String>,
    pub api_base: String,
}

impl NeteaseClient {
    fn api_endpoint(&self, path: &str) -> String {
        format!("{}{}", self.api_base.trim_end_matches('/'), path)
    }

    pub fn set_proxy(&mut self, proxy_url: Option<String>) {
        self.proxy_url = proxy_url;
    }

    fn build_client(&self, timeout_secs: u64) -> Result<Client> {
        let mut builder = Client::builder().timeout(Duration::from_secs(timeout_secs));
        if let Some(proxy_url) = &self.proxy_url {
            builder = builder.proxy(Proxy::all(proxy_url).with_context(|| format!("invalid proxy url: {proxy_url}"))?);
        }
        builder.build().context("failed to build reqwest client")
    }

    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static("NeteaseMusic/2.5.1 (iPhone; iOS 16.6; Scale/3.00)"),
        );
        headers.insert(
            REFERER,
            HeaderValue::from_static("https://music.163.com/"),
        );
        headers.insert(ORIGIN, HeaderValue::from_static("https://music.163.com"));
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/x-www-form-urlencoded"),
        );
        headers
    }

    fn random_device_id() -> String {
        let mut rng = rand::rng();
        let bytes: [u8; 8] = rng.random();
        bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join("")
    }

    fn eapi(path: &str, obj: &Value) -> Result<String> {
        let text = serde_json::to_string(obj).context("failed to serialize eapi payload")?;
        let message = format!("nobody{path}use{text}md5forencrypt");
        let digest = format!("{:x}", md5::compute(message));
        let data = format!("{path}-36cd479b6b5-{text}-36cd479b6b5-{digest}");

        let cipher = Cipher::aes_128_ecb();
        let encrypted = encrypt(cipher, EAPI_KEY.as_bytes(), None, data.as_bytes())
            .context("failed to encrypt eapi payload")?;

        Ok(encrypted
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(""))
    }

    async fn eapi_post(&self, path: &str, endpoint: &str, data: Value) -> Result<Value> {
        let params = Self::eapi(path, &data)?;
        let body = format!("params={params}");

        let client = self.build_client(15)?;
        let response = client
            .post(endpoint)
            .headers(self.headers())
            .body(body)
            .send()
            .await
            .with_context(|| format!("request failed for {endpoint}"))?;

        let status = response.status();
        let text = response.text().await.context("failed reading response body")?;
        if !status.is_success() {
            return Err(anyhow!("netease api HTTP {}: {}", status.as_u16(), text));
        }

        serde_json::from_str(&text).with_context(|| format!("invalid JSON from {endpoint}"))
    }

    pub async fn get_song_info(&self, id: &str) -> Result<Song> {
        let path = "/api/v3/song/detail";
        let payload = json!({
            "c": serde_json::to_string(&vec![json!({"id": id})])?,
            "header": {
                "os": "iOS",
                "appver": "2.5.1",
                "deviceId": Self::random_device_id()
            }
        });

        let v = self
            .eapi_post(path, &self.api_endpoint("/eapi/v3/song/detail"), payload)
            .await?;

        let song = v
            .get("songs")
            .and_then(|s| s.as_array())
            .and_then(|arr| arr.first())
            .ok_or_else(|| anyhow!("song not found for id {id}"))?;

        let id = song.get("id").and_then(|x| x.as_i64()).unwrap_or_default().to_string();
        let name = song.get("name").and_then(|x| x.as_str()).unwrap_or("Unknown Song").to_string();
        let alias = song
            .get("alia")
            .and_then(|a| a.as_array())
            .and_then(|arr| arr.first())
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());

        let full_name = if let Some(a) = alias {
            if a.is_empty() {
                name
            } else {
                format!("{name} ({a})")
            }
        } else {
            name
        };

        let artists = song
            .get("ar")
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|x| Artist {
                        name: x.get("name").and_then(|n| n.as_str()).unwrap_or("Unknown Artist").to_string(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| vec![Artist {
                name: "Unknown Artist".to_string(),
            }]);

        let album = song.get("al").map(|a| Album {
            name: a.get("name").and_then(|n| n.as_str()).unwrap_or_default().to_string(),
            pic_url: a.get("picUrl").and_then(|p| p.as_str()).map(|s| s.to_string()),
        });

        let duration = song.get("dt").and_then(|x| x.as_u64());
        let publish_time = song.get("publishTime").and_then(|x| x.as_u64());

        Ok(Song {
            id,
            name: full_name,
            artists,
            album,
            duration,
            publish_time,
        })
    }

    pub async fn resolve_program_main_song_id(&self, program_id: &str) -> Result<String> {
        let path = "/api/dj/program/detail";
        let payload = json!({
            "id": program_id,
            "header": {
                "os": "iOS",
                "appver": "2.5.1",
                "deviceId": Self::random_device_id()
            }
        });

        let v = self
            .eapi_post(path, &self.api_endpoint("/eapi/dj/program/detail"), payload)
            .await?;

        let main_song_id = v
            .get("program")
            .and_then(|p| p.get("mainSong"))
            .and_then(|s| s.get("id"))
            .and_then(|id| id.as_i64())
            .map(|id| id.to_string())
            .ok_or_else(|| anyhow!("failed to resolve main song id from program {program_id}"))?;

        Ok(main_song_id)
    }

    pub async fn get_album_info(&self, album_id: &str) -> Result<AlbumInfo> {
        let path = format!("/api/v1/album/{album_id}");
        let payload = json!({
            "header": {
                "os": "iOS",
                "appver": "2.5.1",
                "deviceId": Self::random_device_id()
            }
        });

        let endpoint = self.api_endpoint(&format!("/eapi/v1/album/{album_id}"));
        let v = self.eapi_post(&path, &endpoint, payload).await?;

        if v.get("code").and_then(|c| c.as_i64()) != Some(200) {
            let msg = v.get("message").and_then(|m| m.as_str()).unwrap_or("unknown api error");
            return Err(anyhow!("failed to fetch album info: {msg}"));
        }

        let album = v.get("album").ok_or_else(|| anyhow!("album payload missing"))?;
        let album_name = album.get("name").and_then(|x| x.as_str()).unwrap_or_default().to_string();
        let pic_url = album.get("picUrl").and_then(|x| x.as_str()).map(|s| s.to_string());
        let publish_time = album.get("publishTime").and_then(|x| x.as_u64());

        let artist_name = album
            .get("artists")
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.get("name").and_then(|n| n.as_str()))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Unknown Artist".to_string());

        let songs = v
            .get("songs")
            .and_then(|x| x.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|s| {
                        let id = s.get("id").and_then(|x| x.as_i64()).unwrap_or_default().to_string();
                        let name = s.get("name").and_then(|x| x.as_str()).unwrap_or("Unknown Song").to_string();
                        let alias = s
                            .get("alia")
                            .and_then(|a| a.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|x| x.as_str())
                            .map(|x| x.to_string());

                        let full_name = if let Some(a) = alias {
                            if a.is_empty() {
                                name
                            } else {
                                format!("{name} ({a})")
                            }
                        } else {
                            name
                        };

                        let artists = s
                            .get("ar")
                            .and_then(|a| a.as_array())
                            .map(|ars| {
                                ars.iter()
                                    .map(|a| Artist {
                                        name: a
                                            .get("name")
                                            .and_then(|n| n.as_str())
                                            .unwrap_or("Unknown Artist")
                                            .to_string(),
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_else(|| vec![Artist {
                                name: "Unknown Artist".to_string(),
                            }]);

                        Song {
                            id,
                            name: full_name,
                            artists,
                            album: Some(Album {
                                name: album_name.clone(),
                                pic_url: pic_url.clone(),
                            }),
                            duration: s.get("dt").and_then(|x| x.as_u64()),
                            publish_time: s.get("publishTime").and_then(|x| x.as_u64()),
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(AlbumInfo {
            songs,
            album_name,
            artist_name,
            pic_url,
            publish_time,
        })
    }

    async fn get_song_url_by_level(&self, id: &str, level: &str) -> Result<Option<String>> {
        let path = "/api/song/enhance/player/url/v1";
        let payload = json!({
            "ids": [id],
            "level": level,
            "encodeType": "aac",
            "header": {
                "os": "iOS",
                "appver": "2.5.1",
                "deviceId": Self::random_device_id()
            }
        });

        let v = self
            .eapi_post(
                path,
                &self.api_endpoint("/eapi/song/enhance/player/url/v1"),
                payload,
            )
            .await?;

        if v.get("code").and_then(|x| x.as_i64()) != Some(200) {
            return Ok(None);
        }

        let url = v
            .get("data")
            .and_then(|d| d.as_array())
            .and_then(|arr| arr.first())
            .and_then(|x| x.get("url"))
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());

        Ok(url)
    }

    pub async fn check_song_availability(&self, id: &str) -> Result<Availability> {
        let client = self.build_client(10)?;
        for level in QUALITY_LEVELS {
            if let Some(url) = self.get_song_url_by_level(id, level).await? {
                let resp = client
                    .head(&url)
                    .header(REFERER, "https://music.163.com/")
                    .send()
                    .await;

                if let Ok(head) = resp {
                    if !head.status().is_success() && !head.status().is_redirection() {
                        continue;
                    }
                    let len = head
                        .headers()
                        .get("content-length")
                        .and_then(|h| h.to_str().ok())
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0);
                    if len > 500 * 1024 {
                        let file_type = Url::parse(&url)
                            .ok()
                            .and_then(|u| {
                                u.path_segments()
                                    .and_then(|mut seg| seg.next_back())
                                    .and_then(|f| f.split('.').next_back())
                                    .map(|x| x.to_string())
                            })
                            .or_else(|| Some("mp3".to_string()));

                        return Ok(Availability {
                            available: true,
                            content_length: Some(len),
                            url: Some(url),
                            need_proxy: false,
                            quality: Some(level.to_string()),
                            bitrate: Some((len * 8) / 300 / 1000),
                            file_type,
                        });
                    }
                }
            }
        }

        Ok(Availability {
            available: false,
            content_length: None,
            url: None,
            need_proxy: false,
            quality: None,
            bitrate: None,
            file_type: None,
        })
    }

    pub async fn check_song_availability_with_retry(&mut self, id: &str, auto_proxy: bool) -> Result<Availability> {
        let original_proxy = self.proxy_url.clone();

        self.set_proxy(None);
        let direct = self.check_song_availability(id).await?;
        if direct.available {
            return Ok(direct);
        }

        if auto_proxy {
            if let Some(found_proxy) = get_auto_proxy(false).await? {
                self.set_proxy(Some(found_proxy));
                let mut via_proxy = self.check_song_availability(id).await?;
                via_proxy.need_proxy = via_proxy.available;
                if via_proxy.available {
                    return Ok(via_proxy);
                }
            }
        } else if original_proxy.is_some() {
            self.set_proxy(original_proxy.clone());
            let mut preset_proxy = self.check_song_availability(id).await?;
            preset_proxy.need_proxy = preset_proxy.available;
            if preset_proxy.available {
                return Ok(preset_proxy);
            }
        }

        Ok(Availability {
            available: false,
            content_length: None,
            url: None,
            need_proxy: false,
            quality: None,
            bitrate: None,
            file_type: None,
        })
    }

    pub async fn get_lyrics(&self, id: &str) -> Result<Option<String>> {
        let path = "/api/song/lyric/v1";
        let payload = json!({
            "id": id,
            "lv": 1,
            "kv": 1,
            "tv": -1,
            "header": {
                "os": "iOS",
                "appver": "2.5.1",
                "deviceId": Self::random_device_id()
            }
        });

        let v = self
            .eapi_post(path, &self.api_endpoint("/eapi/song/lyric/v1"), payload)
            .await?;

        if v.get("code").and_then(|x| x.as_i64()) != Some(200) {
            return Ok(None);
        }

        Ok(v.get("lrc")
            .and_then(|x| x.get("lyric"))
            .and_then(|x| x.as_str())
            .map(|s| s.to_string()))
    }
}

impl Default for NeteaseClient {
    fn default() -> Self {
        Self {
            proxy_url: None,
            api_base: std::env::var("NETEASE_API_BASE")
                .unwrap_or_else(|_| "https://interface3.music.163.com".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex::Regex;
    use serde_json::json;

    #[test]
    fn eapi_is_deterministic_for_same_input() {
        let payload = json!({"id":"123", "lv":1});
        let p1 = NeteaseClient::eapi("/api/song/lyric/v1", &payload).expect("eapi params");
        let p2 = NeteaseClient::eapi("/api/song/lyric/v1", &payload).expect("eapi params");
        assert_eq!(p1, p2);
    }

    #[test]
    fn eapi_output_is_uppercase_hex() {
        let payload = json!({"id":"123"});
        let params = NeteaseClient::eapi("/api/v3/song/detail", &payload).expect("eapi params");
        let re = Regex::new(r"^[0-9A-F]+$").expect("regex");
        assert!(re.is_match(&params));
        assert_eq!(params.len() % 2, 0);
    }

    #[test]
    fn random_device_id_is_16_upper_hex_chars() {
        let id = NeteaseClient::random_device_id();
        let re = Regex::new(r"^[0-9A-F]{16}$").expect("regex");
        assert!(re.is_match(&id));
    }
}
