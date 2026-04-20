use std::time::{SystemTime, UNIX_EPOCH};
use crate::error::AppError;
use crate::state::SharedState;

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

pub async fn check_rate_limit(state: &SharedState) -> Result<(), AppError> {
    let rate_limit_seconds = match state.rate_limit_seconds {
        Some(s) => s,
        None => return Ok(()),
    };

    let rate_limit_ms = rate_limit_seconds * 1000;
    let now = now_millis();

    let mut last_ts = state.last_request_timestamp.write().await;
    if let Some(last) = *last_ts {
        let elapsed = now - last;
        if elapsed < rate_limit_ms {
            if state.rate_limit_wait {
                let wait_ms = rate_limit_ms - elapsed;
                tracing::info!("Rate limit: waiting {}ms", wait_ms);
                tokio::time::sleep(tokio::time::Duration::from_millis(wait_ms)).await;
            } else {
                return Err(AppError::rate_limited());
            }
        }
    }

    *last_ts = Some(now_millis());
    Ok(())
}
