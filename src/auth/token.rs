use anyhow::Result;
use std::sync::Arc;

use crate::auth::device_flow;
use crate::auth::github;
use crate::config::VSCODE_VERSION_FALLBACK;
use crate::paths;
use crate::state::AppState;

pub async fn setup_github_token(state: &Arc<AppState>, force: bool) -> Result<()> {
    // Try reading existing token
    let token_path = paths::github_token_path();
    if !force {
        if let Ok(token) = std::fs::read_to_string(&token_path) {
            let token = token.trim().to_string();
            if !token.is_empty() {
                tracing::info!("Using existing GitHub token from {}", token_path.display());
                if state.show_token {
                    tracing::info!("GitHub token: {}", token);
                }
                *state.github_token.write().await = Some(token.clone());

                // Verify token by fetching user
                let vs_code_version = state
                    .vs_code_version
                    .read()
                    .await
                    .clone()
                    .unwrap_or_else(|| VSCODE_VERSION_FALLBACK.to_string());
                match github::get_user(&state.http_client, &token, &vs_code_version).await {
                    Ok(user) => {
                        tracing::info!("Logged in as: {}", user.login);
                        return Ok(());
                    }
                    Err(e) => {
                        tracing::warn!("Existing token invalid: {}, re-authenticating...", e);
                    }
                }
            }
        }
    }

    // Run device code flow
    tracing::info!("Starting GitHub authentication...");
    let device_code = device_flow::get_device_code(&state.http_client).await?;

    println!();
    println!("  Please visit: {}", device_code.verification_uri);
    println!("  And enter code: {}", device_code.user_code);
    println!();

    let access_token = device_flow::poll_access_token(
        &state.http_client,
        &device_code.device_code,
        device_code.interval,
    )
    .await?;

    // Save token
    std::fs::write(&token_path, &access_token)?;
    tracing::info!("GitHub token saved to {}", token_path.display());

    if state.show_token {
        tracing::info!("GitHub token: {}", access_token);
    }

    *state.github_token.write().await = Some(access_token.clone());

    // Verify
    let vs_code_version = state
        .vs_code_version
        .read()
        .await
        .clone()
        .unwrap_or_else(|| VSCODE_VERSION_FALLBACK.to_string());
    let user = github::get_user(&state.http_client, &access_token, &vs_code_version).await?;
    tracing::info!("Logged in as: {}", user.login);

    Ok(())
}

pub async fn setup_copilot_token(state: &Arc<AppState>) -> Result<()> {
    let github_token = state
        .github_token
        .read()
        .await
        .clone()
        .ok_or_else(|| anyhow::anyhow!("GitHub token not set"))?;
    let vs_code_version = state
        .vs_code_version
        .read()
        .await
        .clone()
        .unwrap_or_else(|| VSCODE_VERSION_FALLBACK.to_string());

    let token_response =
        github::get_copilot_token(&state.http_client, &github_token, &vs_code_version).await?;

    if state.show_token {
        tracing::info!("Copilot token: {}", token_response.token);
    }

    *state.copilot_token.write().await = Some(token_response.token);
    let refresh_in = token_response.refresh_in.saturating_sub(60);

    tracing::info!("Copilot token acquired, refreshing in {}s", refresh_in);

    // Spawn refresh task
    let state_clone = Arc::clone(state);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(refresh_in)).await;
            tracing::info!("Refreshing Copilot token...");

            let github_token = state_clone.github_token.read().await.clone();
            let vs_code_version = state_clone.vs_code_version.read().await.clone();

            if let (Some(gh_token), Some(vsc_version)) = (github_token, vs_code_version) {
                loop {
                    match github::get_copilot_token(
                        &state_clone.http_client,
                        &gh_token,
                        &vsc_version,
                    )
                    .await
                    {
                        Ok(new_token) => {
                            if state_clone.show_token {
                                tracing::info!("Copilot token refreshed: {}", new_token.token);
                            }
                            *state_clone.copilot_token.write().await = Some(new_token.token);
                            tracing::info!("Copilot token refreshed successfully");
                            break;
                        }
                        Err(e) => {
                            tracing::error!("Failed to refresh Copilot token: {}, retrying in 5s...", e);
                            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                        }
                    }
                }
            } else {
                tracing::error!("Cannot refresh: missing GitHub token or VSCode version");
            }
        }
    });

    Ok(())
}
