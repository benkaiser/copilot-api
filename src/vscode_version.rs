use crate::config::VSCODE_VERSION_FALLBACK;

pub async fn fetch_vscode_version(client: &reqwest::Client) -> String {
    let url = "https://aur.archlinux.org/cgit/aur.git/plain/PKGBUILD?h=visual-studio-code-bin";

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        client.get(url).send(),
    )
    .await;

    match result {
        Ok(Ok(response)) => {
            if let Ok(text) = response.text().await {
                for line in text.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with("pkgver=") {
                        let version = trimmed.trim_start_matches("pkgver=").trim();
                        if !version.is_empty() {
                            tracing::debug!("Fetched VSCode version: {}", version);
                            return version.to_string();
                        }
                    }
                }
            }
            tracing::warn!("Could not parse VSCode version from PKGBUILD, using fallback");
            VSCODE_VERSION_FALLBACK.to_string()
        }
        Ok(Err(e)) => {
            tracing::warn!("Failed to fetch VSCode version: {}, using fallback", e);
            VSCODE_VERSION_FALLBACK.to_string()
        }
        Err(_) => {
            tracing::warn!("VSCode version fetch timed out, using fallback");
            VSCODE_VERSION_FALLBACK.to_string()
        }
    }
}
