use anyhow::Result;
use serde::Deserialize;

use crate::config::GITHUB_CLIENT_ID;

#[derive(Debug, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    #[allow(dead_code)]
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Deserialize)]
pub struct AccessTokenResponse {
    pub access_token: Option<String>,
    #[allow(dead_code)]
    pub token_type: Option<String>,
    #[allow(dead_code)]
    pub scope: Option<String>,
    pub error: Option<String>,
}

pub async fn get_device_code(client: &reqwest::Client) -> Result<DeviceCodeResponse> {
    let response = client
        .post("https://github.com/login/device/code")
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .json(&serde_json::json!({
            "client_id": GITHUB_CLIENT_ID,
            "scope": "read:user"
        }))
        .send()
        .await?
        .json::<DeviceCodeResponse>()
        .await?;

    Ok(response)
}

pub async fn poll_access_token(
    client: &reqwest::Client,
    device_code: &str,
    interval: u64,
) -> Result<String> {
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(interval + 1)).await;

        let response = client
            .post("https://github.com/login/oauth/access_token")
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .json(&serde_json::json!({
                "client_id": GITHUB_CLIENT_ID,
                "device_code": device_code,
                "grant_type": "urn:ietf:params:oauth:grant-type:device_code"
            }))
            .send()
            .await?
            .json::<AccessTokenResponse>()
            .await?;

        if let Some(token) = response.access_token {
            if !token.is_empty() {
                return Ok(token);
            }
        }

        if let Some(error) = &response.error {
            match error.as_str() {
                "authorization_pending" => {
                    tracing::debug!("Authorization pending, polling again...");
                    continue;
                }
                "slow_down" => {
                    tracing::debug!("Polling too fast, slowing down...");
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    continue;
                }
                "expired_token" => {
                    anyhow::bail!("Device code expired. Please try again.");
                }
                "access_denied" => {
                    anyhow::bail!("Access denied by user.");
                }
                other => {
                    anyhow::bail!("OAuth error: {}", other);
                }
            }
        }
    }
}
