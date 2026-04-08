use crate::netease::NeteaseClient;
use crate::proxy::get_auto_proxy;
use crate::utils::{DownloadPath, extract_id, get_download_path, sanitize_extension, sanitize_file_name};
use anyhow::{Context, Result, anyhow};
use futures_util::StreamExt;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use tokio::io::AsyncWriteExt;
use tokio::time::{Duration, timeout};

fn progress_style() -> ProgressStyle {
    ProgressStyle::with_template("{msg} [{bar:30.cyan/blue}] {bytes}/{total_bytes} ({percent}%)")
        .unwrap_or_else(|_| ProgressStyle::default_bar())
}

async fn write_lyrics(path: &DownloadPath, lyrics: &str) -> Result<()> {
    path.write_bytes(lyrics.as_bytes()).await
}

async fn stream_download(client: &reqwest::Client, url: &str, out_path: &DownloadPath, label: &str) -> Result<()> {
    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("failed GET {url}"))?;

    if !resp.status().is_success() {
        return Err(anyhow!("download request failed with status {}", resp.status()));
    }

    let total = resp.content_length().unwrap_or(0);
    let bar = ProgressBar::new(total);
    bar.set_style(progress_style());
    bar.set_message(label.to_string());

    let mut file = out_path.create_file().await?;

    let mut stream = resp.bytes_stream();
    let mut downloaded: u64 = 0;

    while let Some(chunk) = stream.next().await {
        let bytes = chunk.context("failed reading response chunk")?;
        file.write_all(&bytes).await.context("failed writing audio chunk")?;
        downloaded += bytes.len() as u64;
        bar.set_position(downloaded);
    }

    file.flush().await.context("failed flushing file")?;
    bar.finish_with_message(format!("completed: {label}"));

    if total > 0 && downloaded < ((total as f64) * 0.99) as u64 {
        let _ = out_path.remove_file().await;
        return Err(anyhow!("incomplete download"));
    }

    Ok(())
}

pub async fn download_song(
    client: &mut NeteaseClient,
    raw_id: &str,
    auto_proxy: bool,
    program: bool,
) -> Result<()> {
    let id = extract_id(raw_id)?;
    let target_song_id = if program {
        let resolved = client.resolve_program_main_song_id(&id).await?;
        println!("Resolved program {id} -> song {resolved}");
        resolved
    } else {
        id
    };
    let max_retries = 3;

    for attempt in 1..=max_retries {
        let result: Result<()> = async {
            let song = client.get_song_info(&target_song_id).await?;
            let song_name = song.name;
            let artist_name = song
                .artists
                .first()
                .map(|a| a.name.clone())
                .unwrap_or_else(|| "Unknown Artist".to_string());

            println!("Song info: {artist_name}-{song_name}");

            let availability = client
                .check_song_availability_with_retry(&target_song_id, auto_proxy)
                .await?;

            if !availability.available || availability.url.is_none() {
                println!("Song unavailable or no copyright, skipping.");
                return Ok(());
            }

            let audio_url = availability.url.as_ref().expect("checked is_some");
            let ext = sanitize_extension(
                &availability
                    .file_type
                    .clone()
                    .unwrap_or_else(|| "mp3".to_string()),
            );
            println!(
                "Quality: {}, bitrate: {} kbps, format: {}",
                availability.quality.clone().unwrap_or_else(|| "unknown".to_string()),
                availability.bitrate.unwrap_or(0),
                ext
            );

            let safe_song = sanitize_file_name(&song_name);
            let safe_artist = sanitize_file_name(&artist_name);
            let file_name = format!("{safe_artist}-{safe_song}.{ext}");
            let lrc_name = format!("{safe_artist}-{safe_song}.lrc");
            let audio_path = get_download_path("single", &file_name, None)?;
            let lrc_path = get_download_path("single", &lrc_name, None)?;

            if let Some(lyrics) = client.get_lyrics(&target_song_id).await? {
                write_lyrics(&lrc_path, &lyrics).await?;
                println!("Lyrics downloaded: {}", lrc_path.display());
            }

            if audio_path.exists() {
                println!("File exists, skipping: {file_name}");
                return Ok(());
            }

            let req_client = {
                let mut b = reqwest::Client::builder().timeout(Duration::from_secs(30));
                if availability.need_proxy && let Some(proxy_url) = &client.proxy_url {
                    b = b.proxy(reqwest::Proxy::all(proxy_url)?);
                }
                b.build()?
            };

            timeout(
                Duration::from_secs(180),
                stream_download(&req_client, audio_url, &audio_path, &file_name),
            )
            .await
            .context("download timed out")??;

            Ok(())
        }
        .await;

        match result {
            Ok(()) => return Ok(()),
            Err(err) => {
                if attempt == max_retries {
                    return Err(err).context("max retries reached");
                }
                println!("Retry {attempt}/{max_retries} after error: {err}");
                if auto_proxy {
                    let _ = get_auto_proxy(true).await;
                }
            }
        }
    }

    Ok(())
}

pub async fn download_album(client: &mut NeteaseClient, raw_album_id: &str, auto_proxy: bool) -> Result<()> {
    let album_id = extract_id(raw_album_id)?;
    let album = client.get_album_info(&album_id).await?;
    println!("Album info: {} - {}", album.album_name, album.artist_name);
    println!("Total songs: {}", album.songs.len());

    let safe_album = sanitize_file_name(&album.album_name);
    let safe_artist = sanitize_file_name(&album.artist_name);
    let album_dir_name = format!("{safe_artist}-{safe_album}");

    let mp = MultiProgress::new();
    let mut success = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;

    for (i, song) in album.songs.iter().enumerate() {
        let song_name = song.name.clone();
        let artist_name = song
            .artists
            .first()
            .map(|a| a.name.clone())
            .unwrap_or_else(|| "Unknown Artist".to_string());

        let availability = client
            .check_song_availability_with_retry(&song.id, auto_proxy)
            .await?;

        if !availability.available || availability.url.is_none() {
            skipped += 1;
            println!("[{}/{}] skipped (unavailable): {}-{}", i + 1, album.songs.len(), artist_name, song_name);
            continue;
        }

        let ext = sanitize_extension(
            &availability
                .file_type
                .clone()
                .unwrap_or_else(|| "mp3".to_string()),
        );
        println!(
            "[{}/{}] quality: {}, bitrate: {} kbps, format: {}",
            i + 1,
            album.songs.len(),
            availability.quality.clone().unwrap_or_else(|| "unknown".to_string()),
            availability.bitrate.unwrap_or(0),
            ext
        );
        let safe_song = sanitize_file_name(&song_name);
        let safe_song_artist = sanitize_file_name(&artist_name);

        let base = format!("{:02}.{}-{}", i + 1, safe_song_artist, safe_song);
        let file_name = format!("{base}.{ext}");
        let lrc_name = format!("{base}.lrc");

        let audio_path = get_download_path("album", &file_name, Some(&album_dir_name))?;
        let lrc_path = get_download_path("album", &lrc_name, Some(&album_dir_name))?;

        if let Some(lyrics) = client.get_lyrics(&song.id).await? {
            write_lyrics(&lrc_path, &lyrics).await?;
        }

        if audio_path.exists() {
            skipped += 1;
            println!("[{}/{}] file exists, skipping: {}", i + 1, album.songs.len(), file_name);
            continue;
        }

        let label = format!("[{}/{}] {}", i + 1, album.songs.len(), song_name);

        let req_client = {
            let mut b = reqwest::Client::builder().timeout(Duration::from_secs(30));
            if availability.need_proxy && let Some(proxy_url) = &client.proxy_url {
                b = b.proxy(reqwest::Proxy::all(proxy_url)?);
            }
            b.build()?
        };

        let pb = mp.add(ProgressBar::new(availability.content_length.unwrap_or(0)));
        pb.set_style(progress_style());
        pb.set_message(label.clone());

        let res = async {
            let resp = req_client.get(availability.url.as_ref().expect("checked")).send().await?;
            if !resp.status().is_success() {
                return Err(anyhow!("download status: {}", resp.status()));
            }
            let total = resp.content_length().unwrap_or(0);
            pb.set_length(total);

            let mut file = audio_path.create_file().await?;
            let mut downloaded = 0u64;
            let mut stream = resp.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let bytes = chunk?;
                file.write_all(&bytes).await?;
                downloaded += bytes.len() as u64;
                pb.set_position(downloaded);
            }
            file.flush().await?;
            pb.finish_with_message(format!("done: {file_name}"));
            Ok::<(), anyhow::Error>(())
        }
        .await;

        match res {
            Ok(()) => success += 1,
            Err(err) => {
                failed += 1;
                let _ = audio_path.remove_file().await;
                pb.abandon_with_message(format!("failed: {file_name}"));
                println!("[{}/{}] failed: {} ({err})", i + 1, album.songs.len(), file_name);
            }
        }
    }

    println!("Album download completed: success={success}, skipped={skipped}, failed={failed}");
    Ok(())
}

pub async fn download_song_lyrics(client: &NeteaseClient, raw_id: &str, program: bool) -> Result<()> {
    let id = extract_id(raw_id)?;
    let target_song_id = if program {
        client.resolve_program_main_song_id(&id).await?
    } else {
        id
    };
    let song = client.get_song_info(&target_song_id).await?;

    let artist_name = song
        .artists
        .first()
        .map(|a| a.name.clone())
        .unwrap_or_else(|| "Unknown Artist".to_string());

    if let Some(lyrics) = client.get_lyrics(&target_song_id).await? {
        let safe_song = sanitize_file_name(&song.name);
        let safe_artist = sanitize_file_name(&artist_name);
        let file_name = format!("{safe_artist}-{safe_song}.lrc");
        let lrc_path = get_download_path("single", &file_name, None)?;
        write_lyrics(&lrc_path, &lyrics).await?;
        println!("Lyrics downloaded: {}", lrc_path.display());
    } else {
        println!("No lyrics available.");
    }

    Ok(())
}

pub async fn download_album_lyrics(client: &NeteaseClient, raw_album_id: &str) -> Result<()> {
    let album_id = extract_id(raw_album_id)?;
    let album = client.get_album_info(&album_id).await?;
    let safe_album = sanitize_file_name(&album.album_name);
    let safe_artist = sanitize_file_name(&album.artist_name);
    let album_dir_name = format!("{safe_artist}-{safe_album}");

    println!("Album lyrics download: {} - {}", album.album_name, album.artist_name);

    for (i, song) in album.songs.iter().enumerate() {
        let artist_name = song
            .artists
            .first()
            .map(|a| a.name.clone())
            .unwrap_or_else(|| "Unknown Artist".to_string());

        if let Some(lyrics) = client.get_lyrics(&song.id).await? {
            let safe_song = sanitize_file_name(&song.name);
            let safe_song_artist = sanitize_file_name(&artist_name);
            let file_name = format!("{:02}.{}-{}.lrc", i + 1, safe_song_artist, safe_song);
            let lrc_path = get_download_path("album", &file_name, Some(&album_dir_name))?;
            write_lyrics(&lrc_path, &lyrics).await?;
            println!("[{}/{}] lyrics saved: {}", i + 1, album.songs.len(), lrc_path.display());
        } else {
            println!("[{}/{}] no lyrics: {}", i + 1, album.songs.len(), song.name);
        }
    }

    Ok(())
}
