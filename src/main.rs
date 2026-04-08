mod commands;
mod netease;
mod proxy;
mod types;
mod utils;

use anyhow::{Result, anyhow};
use clap::{Parser, Subcommand};
use commands::{download_album, download_album_lyrics, download_song, download_song_lyrics};
use netease::NeteaseClient;
use utils::resolve_input_file_path;

#[derive(Parser, Debug)]
#[command(name = "netease-dl-rs")]
#[command(about = "NetEase Cloud Music Downloader")]
struct Cli {
    #[arg(short = 'p', long = "proxy")]
    proxy: Option<String>,

    #[arg(short = 'a', long = "auto-proxy")]
    auto_proxy: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Download {
        ids: Vec<String>,
        #[arg(short = 'f', long = "file")]
        file: Option<String>,
        #[arg(short = 'P', long = "program", help = "Treat input IDs as DJ program IDs")]
        program: bool,
    },
    Album {
        album_id: String,
    },
    Lyrics {
        ids: Vec<String>,
        #[arg(short = 'f', long = "file")]
        file: Option<String>,
        #[arg(short = 'P', long = "program", help = "Treat input IDs as DJ program IDs")]
        program: bool,
    },
    AlbumLyrics {
        album_id: String,
    },
}

fn load_ids_from_file(path: &str) -> Result<Vec<String>> {
    let path = resolve_input_file_path(path)?;
    let content = path.read_to_string()?;

    Ok(content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|s| s.to_string())
        .collect())
}

fn merge_ids(cli_ids: Vec<String>, file: Option<String>) -> Result<Vec<String>> {
    let mut ids = cli_ids;
    if let Some(file) = file {
        ids.extend(load_ids_from_file(&file)?);
    }
    ids.sort();
    ids.dedup();

    if ids.is_empty() {
        return Err(anyhow!("please provide at least one music ID"));
    }

    Ok(ids)
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let mut client = NeteaseClient::default();
    if let Some(proxy) = cli.proxy {
        client.set_proxy(Some(proxy));
    }

    match cli.command {
        Commands::Download { ids, file, program } => {
            let music_ids = merge_ids(ids, file)?;
            println!("Preparing to download {} songs", music_ids.len());
            for id in music_ids {
                if let Err(err) = download_song(&mut client, &id, cli.auto_proxy, program).await {
                    eprintln!("Download failed for {id}: {err}");
                }
            }
            println!("All download tasks completed.");
        }
        Commands::Album { album_id } => {
            download_album(&mut client, &album_id, cli.auto_proxy).await?;
        }
        Commands::Lyrics { ids, file, program } => {
            let music_ids = merge_ids(ids, file)?;
            println!("Preparing to download lyrics for {} songs", music_ids.len());
            for id in music_ids {
                if let Err(err) = download_song_lyrics(&client, &id, program).await {
                    eprintln!("Lyrics download failed for {id}: {err}");
                }
            }
            println!("All lyrics download tasks completed.");
        }
        Commands::AlbumLyrics { album_id } => {
            download_album_lyrics(&client, &album_id).await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn load_ids_from_file_ignores_comments_and_empty_lines() {
        let dir = tempdir().expect("tempdir");
        let original_dir = std::env::current_dir().expect("current dir");
        std::env::set_current_dir(dir.path()).expect("switch current dir");
        let file = dir.path().join("ids.txt");
        std::fs::write(&file, "123\n# comment\n\n456\n").expect("write ids file");

        let ids = load_ids_from_file("ids.txt").expect("load ids");
        std::env::set_current_dir(original_dir).expect("restore current dir");
        assert_eq!(ids, vec!["123".to_string(), "456".to_string()]);
    }

    #[test]
    fn merge_ids_deduplicates_and_sorts() {
        let dir = tempdir().expect("tempdir");
        let original_dir = std::env::current_dir().expect("current dir");
        std::env::set_current_dir(dir.path()).expect("switch current dir");
        let file = dir.path().join("ids.txt");
        std::fs::write(&file, "3\n2\n").expect("write ids file");

        let merged = merge_ids(
            vec!["2".to_string(), "1".to_string(), "1".to_string()],
            Some("ids.txt".to_string()),
        )
        .expect("merge ids");
        std::env::set_current_dir(original_dir).expect("restore current dir");

        assert_eq!(
            merged,
            vec![
                "1".to_string(),
                "2".to_string(),
                "3".to_string()
            ]
        );
    }

    #[test]
    fn merge_ids_rejects_empty_input() {
        let err = merge_ids(vec![], None).expect_err("empty id list should fail");
        assert!(err.to_string().contains("please provide at least one music ID"));
    }
}
