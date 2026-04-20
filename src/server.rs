use axum::routing::{get, post};
use axum::Router;
use tower_http::cors::CorsLayer;

use crate::anthropic::handler::handle_messages;
use crate::proxy::chat::handle_chat_completions;
use crate::proxy::embeddings::handle_embeddings;
use crate::proxy::models::handle_models;
use crate::proxy::token_route::handle_token;
use crate::proxy::usage::handle_usage;
use crate::state::SharedState;

pub fn create_router(state: SharedState) -> Router {
    Router::new()
        // Health check
        .route("/", get(|| async { "Copilot API proxy is running" }))
        // OpenAI-compatible endpoints
        .route("/chat/completions", post(handle_chat_completions))
        .route("/v1/chat/completions", post(handle_chat_completions))
        .route("/models", get(handle_models))
        .route("/v1/models", get(handle_models))
        .route("/embeddings", post(handle_embeddings))
        .route("/v1/embeddings", post(handle_embeddings))
        // Anthropic-compatible endpoints
        .route("/v1/messages", post(handle_messages))
        // Utility endpoints
        .route("/usage", get(handle_usage))
        .route("/token", get(handle_token))
        // Middleware
        .layer(CorsLayer::permissive())
        .with_state(state)
}
