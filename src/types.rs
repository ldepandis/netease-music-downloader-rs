use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artist {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Album {
    pub name: String,
    pub pic_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Song {
    pub id: String,
    pub name: String,
    pub artists: Vec<Artist>,
    pub album: Option<Album>,
    pub duration: Option<u64>,
    pub publish_time: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlbumInfo {
    pub songs: Vec<Song>,
    pub album_name: String,
    pub artist_name: String,
    pub pic_url: Option<String>,
    pub publish_time: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Availability {
    pub available: bool,
    pub content_length: Option<u64>,
    pub url: Option<String>,
    pub need_proxy: bool,
    pub quality: Option<String>,
    pub bitrate: Option<u64>,
    pub file_type: Option<String>,
}
