use anyhow::{bail, Context, Result};
use directories::ProjectDirs;
use livesplit_core::auto_splitting::list::{Downloader, List, DEFAULT_LIST_URL};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

const LIST_FILE_NAME: &str = "LiveSplit.AutoSplitters.xml";

pub fn cache_dir() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("org", "LiveSplit", "LiveSplit One")
        .context("Application data directory is unavailable")?;
    Ok(dirs.data_local_dir().join("autosplitters"))
}

pub fn is_managed(path: &Path) -> bool {
    cache_dir().is_ok_and(|dir| path.starts_with(dir))
}

pub fn managed_path(game: &str, url: &str) -> Result<PathBuf> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        bail!("Registry URL is not HTTP(S)");
    }
    let filename = Path::new(url)
        .file_name()
        .filter(|name| !name.is_empty())
        .context("Registry URL has no file name")?;
    let safe_game: String = game
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    Ok(cache_dir()?.join(safe_game).join(filename))
}

fn downloader() -> Result<Downloader> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .context("Could not create the HTTP client")?;
    Ok(Downloader::with_client(client))
}

async fn load_cached_or_refresh(downloader: &Downloader) -> Result<String> {
    let cache = cache_dir()?.join(LIST_FILE_NAME);
    let fetched = async {
        let xml = downloader
            .download_list(DEFAULT_LIST_URL)
            .await
            .context("Registry request failed")?;
        // Looking up a game that can't exist scans the whole list, so this
        // rejects malformed responses before they replace the cache.
        List::new(&xml)
            .get_for_game("")
            .context("Malformed auto-splitter registry")?;
        anyhow::Ok(xml)
    }
    .await;
    match fetched {
        Ok(xml) => {
            if let Some(parent) = cache.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&cache, &xml)?;
            Ok(xml)
        }
        Err(error) => fs::read_to_string(&cache)
            .with_context(|| format!("{error:#}; no cached registry is available")),
    }
}

/// Looks up the game in the registry and downloads its auto splitter next to
/// its managed location. Returns the temporary and the final path, so the
/// caller can verify the module loads before installing it.
pub async fn download_for_game(game: String) -> Result<(PathBuf, PathBuf)> {
    let downloader = downloader()?;
    let xml = load_cached_or_refresh(&downloader).await?;
    let url = {
        let entry = List::new(&xml)
            .get_for_game(&game)
            .context("Malformed auto-splitter registry")?
            .with_context(|| format!("No registry entry matches {game}"))?;
        if !entry.is_using_auto_splitting_runtime() {
            bail!("The matching entry is a Windows-only auto splitter");
        }
        entry
            .urls()
            .first()
            .context("Registry entry has no download URL")?
            .to_string()
    };
    let destination = managed_path(&game, &url)?;
    let parent = destination
        .parent()
        .context("Managed auto-splitter has no parent directory")?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(".autosplitter.download");
    let bytes = downloader
        .download_file(&url)
        .await
        .context("Auto-splitter download failed")?;
    fs::write(&temporary, bytes)?;
    Ok((temporary, destination))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_http_downloads_and_sanitizes_paths() {
        assert!(managed_path("Game", "file:///tmp/a.wasm").is_err());
        let path = managed_path("../Game Name", "https://example.com/a.wasm").unwrap();
        assert_eq!(path.file_name().unwrap(), "a.wasm");
        assert!(path.to_string_lossy().contains("___Game_Name"));
        assert!(is_managed(&path));
    }
}
