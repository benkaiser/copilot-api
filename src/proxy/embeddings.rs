use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;

use crate::auth::token::refresh_copilot_token;
use crate::config::{copilot_base_url, copilot_headers, VSCODE_VERSION_FALLBACK};
use crate::error::{forward_error, AppError};
use crate::state::SharedState;

pub async fn handle_embeddings(
    State(state): State<SharedState>,
    axum::Json(payload): axum::Json<serde_json::Value>,
) -> Result<Response, AppError> {
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
    let url = format!("{}/embeddings", base_url);
    let headers = copilot_headers(&copilot_token, &vs_code_version, false);

    let mut request = state.http_client.post(&url);
    for (key, value) in &headers {
        request = request.header(key, value);
    }
    request = request.json(&payload);

    let mut response = request.send().await?;

    // On 401, refresh the token and retry once
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        tracing::warn!("Got 401 from upstream (embeddings), attempting token refresh and retry");
        match refresh_copilot_token(&state, &copilot_token).await {
            Ok(new_token) => {
                let retry_headers = copilot_headers(&new_token, &vs_code_version, false);
                let mut retry_request = state.http_client.post(&url);
                for (key, value) in &retry_headers {
                    retry_request = retry_request.header(key, value);
                }
                retry_request = retry_request.json(&payload);

                response = retry_request.send().await?;
                if !response.status().is_success() {
                    return Err(forward_error(response).await);
                }
            }
            Err(e) => {
                tracing::error!("Failed to refresh token on 401: {}", e);
                return Err(forward_error(response).await);
            }
        }
    } else if !response.status().is_success() {
        return Err(forward_error(response).await);
    }

    let body = response.text().await?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap())
}
