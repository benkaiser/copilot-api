use axum::extract::State;
use axum::response::IntoResponse;

use crate::auth::github;
use crate::config::VSCODE_VERSION_FALLBACK;
use crate::error::AppError;
use crate::state::SharedState;

pub async fn handle_usage(State(state): State<SharedState>) -> Result<impl IntoResponse, AppError> {
    let github_token = state
        .github_token
        .read()
        .await
        .clone()
        .ok_or_else(|| AppError::internal("GitHub token not available"))?;
    let vs_code_version = state
        .vs_code_version
        .read()
        .await
        .clone()
        .unwrap_or_else(|| VSCODE_VERSION_FALLBACK.to_string());

    let usage = github::get_copilot_usage(
        &state.http_client,
        &github_token,
        &vs_code_version,
        &state.account_type,
    )
    .await
    .map_err(|e| AppError::internal(e.to_string()))?;

    Ok(axum::Json(usage))
}
