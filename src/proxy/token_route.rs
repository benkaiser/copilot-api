use axum::extract::State;
use axum::response::IntoResponse;

use crate::error::AppError;
use crate::state::SharedState;

pub async fn handle_token(State(state): State<SharedState>) -> Result<impl IntoResponse, AppError> {
    let copilot_token = state
        .copilot_token
        .read()
        .await
        .clone()
        .ok_or_else(|| AppError::internal("Copilot token not available"))?;

    Ok(axum::Json(serde_json::json!({
        "token": copilot_token
    })))
}
