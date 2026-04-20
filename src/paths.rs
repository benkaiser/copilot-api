use anyhow::Result;
use std::fs;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

pub fn app_dir() -> PathBuf {
    // Use ~/.local/share/copilot-api for compatibility with the TS version
    // (dirs::data_local_dir() returns ~/Library/Application Support on macOS)
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
    home.join(".local").join("share").join("copilot-api")
}

pub fn github_token_path() -> PathBuf {
    app_dir().join("github_token")
}

pub fn ensure_paths() -> Result<()> {
    let dir = app_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }

    let token_path = github_token_path();
    if !token_path.exists() {
        fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&token_path)?;
    }

    Ok(())
}
