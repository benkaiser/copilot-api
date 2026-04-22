use axum::extract::State;
use axum::response::IntoResponse;

use crate::auth::token::refresh_copilot_token;
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

    // On 401, refresh the token and retry once
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        tracing::warn!("Got 401 from upstream (models), attempting token refresh and retry");
        match refresh_copilot_token(&state, &copilot_token).await {
            Ok(new_token) => {
                let retry_headers = copilot_headers(&new_token, &vs_code_version, false);
                let mut retry_request = state.http_client.get(&url);
                for (key, value) in &retry_headers {
                    retry_request = retry_request.header(key, value);
                }

                let retry_response = retry_request.send().await?;
                if !retry_response.status().is_success() {
                    return Err(forward_error(retry_response).await);
                }

                let body: serde_json::Value = retry_response.json().await?;
                return Ok(axum::Json(body));
            }
            Err(e) => {
                tracing::error!("Failed to refresh token on 401: {}", e);
                return Err(forward_error(response).await);
            }
        }
    }

    if !response.status().is_success() {
        return Err(forward_error(response).await);
    }

    let body: serde_json::Value = response.json().await?;
    Ok(axum::Json(body))
}
