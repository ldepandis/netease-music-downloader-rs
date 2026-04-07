use std::process::Command;

use assert_cmd::cargo::cargo_bin;
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn mount_song_detail(server: &MockServer, song_id: i64, song_name: &str, artist_name: &str) {
    let body = serde_json::json!({
        "songs": [{
            "id": song_id,
            "name": song_name,
            "alia": [],
            "ar": [{"name": artist_name}],
            "al": {"name": "Mock Album", "picUrl": ""},
            "dt": 123000,
            "publishTime": 0
        }]
    });

    Mock::given(method("POST"))
        .and(path("/eapi/v3/song/detail"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

async fn mount_album_detail(
    server: &MockServer,
    album_id: &str,
    album_name: &str,
    album_artist: &str,
    song_id: i64,
    song_name: &str,
    song_artist: &str,
) {
    let body = serde_json::json!({
        "code": 200,
        "album": {
            "name": album_name,
            "picUrl": "",
            "publishTime": 0,
            "artists": [{"name": album_artist}]
        },
        "songs": [{
            "id": song_id,
            "name": song_name,
            "alia": [],
            "ar": [{"name": song_artist}],
            "dt": 123000,
            "publishTime": 0
        }]
    });

    Mock::given(method("POST"))
        .and(path(format!("/eapi/v1/album/{album_id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

async fn mount_player_url(server: &MockServer, media_url: &str) {
    let body = serde_json::json!({
        "code": 200,
        "data": [{
            "url": media_url
        }]
    });

    Mock::given(method("POST"))
        .and(path("/eapi/song/enhance/player/url/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

async fn mount_program_detail(server: &MockServer, program_id: i64, main_song_id: i64) {
    let body = serde_json::json!({
        "code": 200,
        "program": {
            "id": program_id,
            "mainSong": {
                "id": main_song_id
            }
        }
    });

    Mock::given(method("POST"))
        .and(path("/eapi/dj/program/detail"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

async fn mount_lyrics(server: &MockServer, lyric: &str) {
    let body = serde_json::json!({
        "code": 200,
        "lrc": {"lyric": lyric}
    });

    Mock::given(method("POST"))
        .and(path("/eapi/song/lyric/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

async fn mount_media(server: &MockServer, media_path: &str) {
    Mock::given(method("HEAD"))
        .and(path(media_path))
        .respond_with(ResponseTemplate::new(200).insert_header("content-length", "600000"))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path(media_path))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "10")
                .set_body_string("0123456789"),
        )
        .mount(server)
        .await;
}

fn run_bin(args: &[&str], cwd: &std::path::Path, api_base: &str) {
    let bin = cargo_bin("netease_music_downloader_rs");
    let status = Command::new(bin)
        .args(args)
        .current_dir(cwd)
        .env("NETEASE_API_BASE", api_base)
        .status()
        .expect("failed to execute binary");

    assert!(status.success());
}

#[tokio::test]
async fn download_command_creates_audio_and_lyrics_files() {
    let server = MockServer::start().await;
    let media_path = "/media/song1.mp3";
    let media_url = format!("{}{}", server.uri(), media_path);

    mount_song_detail(&server, 1001, "Song One", "Artist One").await;
    mount_player_url(&server, &media_url).await;
    mount_lyrics(&server, "[00:00.00]Hello").await;
    mount_media(&server, media_path).await;

    let dir = tempdir().expect("temp dir");
    run_bin(&["download", "1001"], dir.path(), &server.uri());

    let audio = dir.path().join("downloads").join("Artist One-Song One.mp3");
    let lrc = dir.path().join("downloads").join("Artist One-Song One.lrc");

    assert!(audio.exists(), "audio file should exist");
    assert!(lrc.exists(), "lyrics file should exist");
    let lyric_body = std::fs::read_to_string(lrc).expect("read lrc");
    assert!(lyric_body.contains("Hello"));
}

#[tokio::test]
async fn lyrics_command_creates_only_lyrics_file() {
    let server = MockServer::start().await;

    mount_song_detail(&server, 1002, "Song Two", "Artist Two").await;
    mount_lyrics(&server, "[00:00.00]Only lyrics").await;

    let dir = tempdir().expect("temp dir");
    run_bin(&["lyrics", "1002"], dir.path(), &server.uri());

    let base = dir.path().join("downloads");
    let lrc = base.join("Artist Two-Song Two.lrc");
    let audio = base.join("Artist Two-Song Two.mp3");

    assert!(lrc.exists(), "lyrics file should exist");
    assert!(!audio.exists(), "audio file must not be created by lyrics command");
}

#[tokio::test]
async fn album_command_creates_indexed_album_files() {
    let server = MockServer::start().await;
    let media_path = "/media/album-track.mp3";
    let media_url = format!("{}{}", server.uri(), media_path);

    mount_album_detail(
        &server,
        "999",
        "Album Alpha",
        "Album Artist",
        2001,
        "Track A",
        "Track Artist",
    )
    .await;
    mount_player_url(&server, &media_url).await;
    mount_lyrics(&server, "[00:00.00]Album lyric").await;
    mount_media(&server, media_path).await;

    let dir = tempdir().expect("temp dir");
    run_bin(&["album", "999"], dir.path(), &server.uri());

    let album_dir = dir
        .path()
        .join("downloads")
        .join("Album Artist-Album Alpha");
    let audio = album_dir.join("01.Track Artist-Track A.mp3");
    let lrc = album_dir.join("01.Track Artist-Track A.lrc");

    assert!(audio.exists(), "album track audio should exist");
    assert!(lrc.exists(), "album track lyrics should exist");
}

#[tokio::test]
async fn download_program_flag_resolves_main_song_and_downloads() {
    let server = MockServer::start().await;
    let media_path = "/media/program-song.mp3";
    let media_url = format!("{}{}", server.uri(), media_path);

    mount_program_detail(&server, 3074011314, 333444555).await;
    mount_song_detail(&server, 333444555, "Program Song", "Program Artist").await;
    mount_player_url(&server, &media_url).await;
    mount_lyrics(&server, "[00:00.00]Program lyric").await;
    mount_media(&server, media_path).await;

    let dir = tempdir().expect("temp dir");
    run_bin(
        &["download", "--program", "3074011314"],
        dir.path(),
        &server.uri(),
    );

    let audio = dir
        .path()
        .join("downloads")
        .join("Program Artist-Program Song.mp3");
    let lrc = dir
        .path()
        .join("downloads")
        .join("Program Artist-Program Song.lrc");

    assert!(audio.exists(), "program audio should exist");
    assert!(lrc.exists(), "program lyrics should exist");
}
