use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use futures::StreamExt;

use crate::auth::token::refresh_copilot_token;
use crate::config::{copilot_base_url, copilot_headers, VSCODE_VERSION_FALLBACK};
use crate::error::{forward_error, AppError};
use crate::openai_types::{ChatCompletionsPayload, ContentPart, MessageContent};
use crate::rate_limit::check_rate_limit;
use crate::state::SharedState;

fn has_vision(payload: &ChatCompletionsPayload) -> bool {
    for msg in &payload.messages {
        if let Some(MessageContent::Parts(parts)) = &msg.content {
            for part in parts {
                if matches!(part, ContentPart::ImageUrl { .. }) {
                    return true;
                }
            }
        }
    }
    false
}

fn get_initiator(payload: &ChatCompletionsPayload) -> &str {
    for msg in &payload.messages {
        if msg.role == "assistant" || msg.role == "tool" {
            return "agent";
        }
    }
    "user"
}

pub async fn handle_chat_completions(
    State(state): State<SharedState>,
    axum::Json(mut payload): axum::Json<ChatCompletionsPayload>,
) -> Result<Response, AppError> {
    check_rate_limit(&state).await?;

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

    // Set max_tokens from model capabilities if not set
    if payload.max_tokens.is_none() {
        if let Some(models) = state.models.read().await.as_ref() {
            if let Some(model) = models.data.iter().find(|m| m.id == payload.model) {
                if let Some(caps) = &model.capabilities {
                    if let Some(limits) = &caps.limits {
                        payload.max_tokens = limits.max_output_tokens;
                    }
                }
            }
        }
    }

    let vision = has_vision(&payload);
    let initiator = get_initiator(&payload);
    let is_stream = payload.stream.unwrap_or(false);
    let base_url = copilot_base_url(&state.account_type);
    let url = format!("{}/chat/completions", base_url);

    let mut headers = copilot_headers(&copilot_token, &vs_code_version, vision);
    headers.push(("X-Initiator".to_string(), initiator.to_string()));

    let mut request = state.http_client.post(&url);
    for (key, value) in &headers {
        request = request.header(key, value);
    }
    request = request.json(&payload);

    let mut response = request.send().await?;

    // On 401, refresh the token and retry once
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        tracing::warn!("Got 401 from upstream, attempting token refresh and retry");
        match refresh_copilot_token(&state, &copilot_token).await {
            Ok(new_token) => {
                let mut retry_headers = copilot_headers(&new_token, &vs_code_version, vision);
                retry_headers.push(("X-Initiator".to_string(), initiator.to_string()));

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

    if is_stream {
        // Stream SSE response through
        let stream = response.bytes_stream().map(|result| {
            result.map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
            })
        });

        let body = Body::from_stream(stream);
        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .header("cache-control", "no-cache")
            .header("connection", "keep-alive")
            .body(body)
            .unwrap())
    } else {
        let body = response.text().await?;
        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap())
    }
}

