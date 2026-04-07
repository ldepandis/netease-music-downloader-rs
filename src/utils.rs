use anyhow::{Context, Result, anyhow};
use regex::Regex;
use std::path::{Path, PathBuf};

pub fn sanitize_file_name(name: &str) -> String {
    name.chars()
        .filter(|c| !matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'))
        .collect::<String>()
        .trim()
        .to_string()
}

pub fn extract_id(input: &str) -> Result<String> {
    if input.chars().all(|c| c.is_ascii_digit()) {
        return Ok(input.to_string());
    }

    let re = Regex::new(r"id=(\d+)").expect("regex must compile");
    if let Some(caps) = re.captures(input)
        && let Some(id) = caps.get(1)
    {
        return Ok(id.as_str().to_string());
    }

    Err(anyhow!("Invalid ID or URL: {input}"))
}

pub fn ensure_dir(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path).with_context(|| format!("failed creating directory: {}", path.display()))
}

pub fn download_base_dir() -> Result<PathBuf> {
    let dir = std::env::current_dir()
        .context("failed to read current dir")?
        .join("downloads");
    ensure_dir(&dir)?;
    Ok(dir)
}

pub fn get_download_path(kind: &str, file_name: &str, album_name: Option<&str>) -> Result<PathBuf> {
    let base = download_base_dir()?;
    if kind == "album"
        && let Some(album) = album_name
    {
        let album_dir = base.join(album);
        ensure_dir(&album_dir)?;
        return Ok(album_dir.join(file_name));
    }
    Ok(base.join(file_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_file_name_removes_invalid_chars() {
        let input = "A:/B*?C\"<D>|\\E";
        let sanitized = sanitize_file_name(input);
        assert_eq!(sanitized, "ABCDE");
    }

    #[test]
    fn extract_id_accepts_numeric() {
        let id = extract_id("123456").expect("numeric id should be valid");
        assert_eq!(id, "123456");
    }

    #[test]
    fn extract_id_accepts_netease_url() {
        let id = extract_id("https://music.163.com/#/song?id=426832090")
            .expect("url with id query must be parsed");
        assert_eq!(id, "426832090");
    }

    #[test]
    fn extract_id_rejects_invalid_input() {
        let err = extract_id("https://music.163.com/#/song")
            .expect_err("missing id query must return error");
        assert!(err.to_string().contains("Invalid ID or URL"));
    }
}
