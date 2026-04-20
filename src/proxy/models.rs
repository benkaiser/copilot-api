use axum::extract::State;
use axum::response::IntoResponse;

use crate::config::{copilot_base_url, copilot_headers, VSCODE_VERSION_FALLBACK};
use crate::error::{forward_error, AppError};
use crate::state::SharedState;

pub async fn handle_models(State(state): State<SharedState>) -> Result<impl IntoResponse, AppError> {
    // Return cached models if available
    if let Some(models) = state.models.read().await.as_ref() {
        return Ok(axum::Json(serde_json::to_value(models).unwrap()));
    }

    let copilot_token = state
        .copilot_token
        .read()
        .await
        .clone()
        .ok_or_else(|| AppError::internal("Copilot token not available"))?;
    let vs_code_version = state
        .vs_code_version
        .read()
        .await
        .clone()
        .unwrap_or_else(|| VSCODE_VERSION_FALLBACK.to_string());

    let base_url = copilot_base_url(&state.account_type);
    let url = format!("{}/models", base_url);
    let headers = copilot_headers(&copilot_token, &vs_code_version, false);

    let mut request = state.http_client.get(&url);
    for (key, value) in &headers {
        request = request.header(key, value);
    }

    let response = request.send().await?;
    if !response.status().is_success() {
        return Err(forward_error(response).await);
    }

    let body: serde_json::Value = response.json().await?;
    Ok(axum::Json(body))
}
