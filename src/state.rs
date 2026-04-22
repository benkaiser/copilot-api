use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use crate::openai_types::ModelsResponse;

#[derive(Debug)]
pub struct AppState {
    pub github_token: RwLock<Option<String>>,
    pub copilot_token: RwLock<Option<String>>,
    pub account_type: String,
    pub models: RwLock<Option<ModelsResponse>>,
    pub vs_code_version: RwLock<Option<String>>,
    pub rate_limit_seconds: Option<u64>,
    pub rate_limit_wait: bool,
    pub show_token: bool,
    pub manual_approve: bool,
    pub last_request_timestamp: RwLock<Option<u64>>,
    pub http_client: reqwest::Client,
    /// Lock to coordinate concurrent token refreshes (prevents thundering herd)
    pub token_refresh_lock: Mutex<()>,
}

pub type SharedState = Arc<AppState>;

impl AppState {
    pub fn new(account_type: String) -> Self {
        Self {
            github_token: RwLock::new(None),
            copilot_token: RwLock::new(None),
            account_type,
            models: RwLock::new(None),
            vs_code_version: RwLock::new(None),
            rate_limit_seconds: None,
            rate_limit_wait: false,
            show_token: false,
            manual_approve: false,
            last_request_timestamp: RwLock::new(None),
            http_client: reqwest::Client::new(),
            token_refresh_lock: Mutex::new(()),
        }
    }
}
