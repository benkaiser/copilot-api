use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use futures::StreamExt;

use crate::anthropic::stream::{format_sse_event, translate_chunk_to_anthropic_events, StreamState};
use crate::anthropic::translate_request::translate_to_openai;
use crate::anthropic::translate_response::translate_to_anthropic;
use crate::anthropic::types::AnthropicMessagesPayload;
use crate::config::{copilot_base_url, copilot_headers, VSCODE_VERSION_FALLBACK};
use crate::error::{forward_error, AppError};
use crate::openai_types::{ChatCompletionChunk, ChatCompletionResponse, ContentPart, MessageContent};
use crate::rate_limit::check_rate_limit;
use crate::state::SharedState;

fn has_vision_openai(messages: &[crate::openai_types::OpenAIMessage]) -> bool {
    for msg in messages {
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

fn get_initiator_openai(messages: &[crate::openai_types::OpenAIMessage]) -> &str {
    for msg in messages {
        if msg.role == "assistant" || msg.role == "tool" {
            return "agent";
        }
    }
    "user"
}

pub async fn handle_messages(
    State(state): State<SharedState>,
    headers: HeaderMap,
    axum::Json(mut payload): axum::Json<AnthropicMessagesPayload>,
) -> Result<Response, AppError> {
    check_rate_limit(&state).await?;

    // Check anthropic-beta header for context-1m
    if let Some(beta_header) = headers.get("anthropic-beta") {
        if let Ok(beta_str) = beta_header.to_str() {
            if beta_str.contains("context-1m") && !payload.model.ends_with("-1m") {
                payload.model = format!("{}-1m", payload.model);
            }
        }
    }

    let original_model = payload.model.clone();
    let is_stream = payload.stream.unwrap_or(false);

    // Translate Anthropic → OpenAI
    let mut openai_payload = translate_to_openai(&payload);
    tracing::info!(
        "Anthropic model {} → OpenAI model {}",
        original_model,
        openai_payload.model
    );

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

    let vision = has_vision_openai(&openai_payload.messages);
    let initiator = get_initiator_openai(&openai_payload.messages);
    let base_url = copilot_base_url(&state.account_type);
    let url = format!("{}/chat/completions", base_url);

    // Set max_tokens from model capabilities if needed
    if openai_payload.max_tokens.is_none() {
        if let Some(models) = state.models.read().await.as_ref() {
            if let Some(model) = models.data.iter().find(|m| m.id == openai_payload.model) {
                if let Some(caps) = &model.capabilities {
                    if let Some(limits) = &caps.limits {
                        openai_payload.max_tokens = limits.max_output_tokens;
                    }
                }
            }
        }
    }

    // Ensure stream flag is set correctly
    openai_payload.stream = Some(is_stream);

    let mut req_headers = copilot_headers(&copilot_token, &vs_code_version, vision);
    req_headers.push(("X-Initiator".to_string(), initiator.to_string()));

    let mut request = state.http_client.post(&url);
    for (key, value) in &req_headers {
        request = request.header(key, value);
    }
    request = request.json(&openai_payload);

    let response = request.send().await?;

    if !response.status().is_success() {
        return Err(forward_error(response).await);
    }

    if is_stream {
        // Streaming: translate OpenAI SSE → Anthropic SSE
        let original_model_clone = original_model.clone();

        let stream = async_stream::stream! {
            let mut stream_state = StreamState::new(&original_model_clone);
            let mut byte_stream = response.bytes_stream();
            let mut buffer = String::new();

            while let Some(chunk_result) = byte_stream.next().await {
                match chunk_result {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes);
                        buffer.push_str(&text);

                        // Process complete SSE lines
                        while let Some(pos) = buffer.find("\n\n") {
                            let line_block = buffer[..pos].to_string();
                            buffer = buffer[pos + 2..].to_string();

                            for line in line_block.lines() {
                                let line = line.trim();
                                if line.starts_with("data: ") {
                                    let data = &line[6..];
                                    if data == "[DONE]" {
                                        continue;
                                    }
                                    if let Ok(chunk) = serde_json::from_str::<ChatCompletionChunk>(data) {
                                        let events = translate_chunk_to_anthropic_events(&chunk, &mut stream_state);
                                        for event in events {
                                            let sse = format_sse_event(&event);
                                            yield Ok::<_, std::io::Error>(bytes::Bytes::from(sse));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Stream error: {}", e);
                        break;
                    }
                }
            }

            // Process any remaining buffer
            if !buffer.is_empty() {
                for line in buffer.lines() {
                    let line = line.trim();
                    if line.starts_with("data: ") {
                        let data = &line[6..];
                        if data != "[DONE]" {
                            if let Ok(chunk) = serde_json::from_str::<ChatCompletionChunk>(data) {
                                let events = translate_chunk_to_anthropic_events(&chunk, &mut stream_state);
                                for event in events {
                                    let sse = format_sse_event(&event);
                                    yield Ok::<_, std::io::Error>(bytes::Bytes::from(sse));
                                }
                            }
                        }
                    }
                }
            }
        };

        let body = Body::from_stream(stream);
        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .header("cache-control", "no-cache")
            .header("connection", "keep-alive")
            .body(body)
            .unwrap())
    } else {
        // Non-streaming: translate OpenAI JSON → Anthropic JSON
        let response_text = response.text().await?;
        let openai_response: ChatCompletionResponse = match serde_json::from_str(&response_text) {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(
                    "Failed to parse OpenAI response: {}. Body: {}",
                    e,
                    &response_text[..response_text.len().min(500)]
                );
                return Err(AppError::internal(format!("Failed to parse upstream response: {}", e)));
            }
        };
        let anthropic_response = translate_to_anthropic(&openai_response, &original_model);

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_string(&anthropic_response).unwrap(),
            ))
            .unwrap())
    }
}
