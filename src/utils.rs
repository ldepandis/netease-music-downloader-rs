use anyhow::{Context, Result, anyhow};
use regex::Regex;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputFilePath {
    inner: PathBuf,
}

impl InputFilePath {
    pub fn resolve(raw_path: &str) -> Result<Self> {
        if raw_path.is_empty() {
            return Err(anyhow!("input file path cannot be empty"));
        }

        if is_absolute_input_path(raw_path) {
            return Err(anyhow!("input file path must be relative to the current directory"));
        }

        reject_parent_traversal(raw_path, "input file path")?;
        let full_path = std::env::current_dir()
            .context("failed to read current dir")?
            .join(raw_path);

        let metadata = std::fs::metadata(&full_path)
            .with_context(|| format!("failed reading metadata for input file: {}", full_path.display()))?;
        if !metadata.is_file() {
            return Err(anyhow!("input path is not a regular file: {}", full_path.display()));
        }

        Ok(Self { inner: full_path })
    }

    pub fn read_to_string(&self) -> Result<String> {
        std::fs::read_to_string(&self.inner)
            .with_context(|| format!("failed reading id file: {}", self.inner.display()))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadPath {
    inner: PathBuf,
}

impl DownloadPath {
    fn new(inner: PathBuf) -> Self {
        Self { inner }
    }

    pub fn display(&self) -> std::path::Display<'_> {
        self.inner.display()
    }

    pub fn exists(&self) -> bool {
        self.inner.exists()
    }

    pub async fn create_file(&self) -> Result<tokio::fs::File> {
        tokio::fs::File::create(&self.inner)
            .await
            .with_context(|| format!("failed creating file: {}", self.inner.display()))
    }

    pub async fn write_bytes(&self, bytes: &[u8]) -> Result<()> {
        tokio::fs::write(&self.inner, bytes)
            .await
            .with_context(|| format!("failed writing file: {}", self.inner.display()))
    }

    pub async fn remove_file(&self) -> Result<()> {
        tokio::fs::remove_file(&self.inner)
            .await
            .with_context(|| format!("failed removing file: {}", self.inner.display()))
    }
}

pub fn sanitize_file_name(name: &str) -> String {
    name.chars()
        .filter(|c| !matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'))
        .collect::<String>()
        .trim()
        .to_string()
}

pub fn sanitize_extension(ext: &str) -> String {
    let sanitized = ext
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();

    if sanitized.is_empty() {
        "mp3".to_string()
    } else {
        sanitized
    }
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

fn validate_single_path_component(component: &str, label: &str) -> Result<()> {
    if component.is_empty() {
        return Err(anyhow!("{label} cannot be empty"));
    }

    if component == "." || component == ".." {
        return Err(anyhow!("{label} cannot be '.' or '..'"));
    }

    if component.contains('/') || component.contains('\\') {
        return Err(anyhow!("{label} must not contain path separators"));
    }

    #[cfg(windows)]
    if component.contains(':') {
        return Err(anyhow!("{label} must not contain drive prefixes"));
    }

    Ok(())
}

fn reject_parent_traversal(raw_path: &str, label: &str) -> Result<()> {
    for part in raw_path.split(['/', '\\']) {
        if part == ".." {
            return Err(anyhow!("{label} cannot contain '..'"));
        }
    }

    Ok(())
}

fn is_absolute_input_path(raw_path: &str) -> bool {
    raw_path.starts_with('/') || raw_path.starts_with('\\') || has_windows_drive_prefix(raw_path)
}

fn has_windows_drive_prefix(raw_path: &str) -> bool {
    let bytes = raw_path.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
}

pub fn resolve_input_file_path(path: &str) -> Result<InputFilePath> {
    InputFilePath::resolve(path)
}

pub fn get_download_path(kind: &str, file_name: &str, album_name: Option<&str>) -> Result<DownloadPath> {
    let base = download_base_dir()?;
    validate_single_path_component(file_name, "file name")?;
    if kind == "album"
        && let Some(album) = album_name
    {
        validate_single_path_component(album, "album directory")?;
        let album_dir = base.join(album);
        ensure_dir(&album_dir)?;
        return Ok(DownloadPath::new(album_dir.join(file_name)));
    }
    Ok(DownloadPath::new(base.join(file_name)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn sanitize_file_name_removes_invalid_chars() {
        let input = "A:/B*?C\"<D>|\\E";
        let sanitized = sanitize_file_name(input);
        assert_eq!(sanitized, "ABCDE");
    }

    #[test]
    fn sanitize_extension_removes_non_alphanumeric_chars() {
        let sanitized = sanitize_extension("../M-P3?");
        assert_eq!(sanitized, "mp3");
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

    #[test]
    fn get_download_path_rejects_nested_file_names() {
        let err = get_download_path("single", "../bad.mp3", None).expect_err("nested path must fail");
        assert!(err.to_string().contains("path separators"));
    }

    #[test]
    fn resolve_input_file_path_rejects_absolute_files() {
        let dir = tempdir().expect("tempdir");
        let file = dir.path().join("ids.txt");
        std::fs::write(&file, "123\n").expect("write file");

        let err = resolve_input_file_path(file.to_str().expect("utf8 path"))
            .expect_err("absolute path must fail");
        assert!(err
            .to_string()
            .contains("must be relative to the current directory"));
    }

    #[test]
    fn resolve_input_file_path_rejects_relative_parent_traversal() {
        let err = resolve_input_file_path("../ids.txt").expect_err("traversal path must fail");
        assert!(err.to_string().contains("cannot contain '..'"));
    }
}
