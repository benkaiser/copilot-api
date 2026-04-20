use anyhow::Result;
use serde::Deserialize;

use crate::config::github_headers;

#[derive(Debug, Deserialize)]
pub struct GitHubUser {
    pub login: String,
}

#[derive(Debug, Deserialize)]
pub struct CopilotTokenResponse {
    pub token: String,
    #[allow(dead_code)]
    pub expires_at: u64,
    pub refresh_in: u64,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct CopilotUsageResponse {
    #[serde(flatten)]
    pub data: serde_json::Value,
}

pub async fn get_user(
    client: &reqwest::Client,
    github_token: &str,
    vs_code_version: &str,
) -> Result<GitHubUser> {
    let headers = github_headers(github_token, vs_code_version);
    let mut request = client.get("https://api.github.com/user");
    for (key, value) in &headers {
        request = request.header(key, value);
    }
    let response = request.send().await?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("GitHub API error ({}): {}", status, text);
    }
    let user = response.json::<GitHubUser>().await?;
    Ok(user)
}

pub async fn get_copilot_token(
    client: &reqwest::Client,
    github_token: &str,
    vs_code_version: &str,
) -> Result<CopilotTokenResponse> {
    let headers = github_headers(github_token, vs_code_version);
    let mut request = client.get("https://api.github.com/copilot_internal/v2/token");
    for (key, value) in &headers {
        request = request.header(key, value);
    }
    let response = request.send().await?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to get Copilot token ({}): {}", status, text);
    }
    let token_response = response.json::<CopilotTokenResponse>().await?;
    Ok(token_response)
}

pub async fn get_copilot_usage(
    client: &reqwest::Client,
    github_token: &str,
    vs_code_version: &str,
    account_type: &str,
) -> Result<serde_json::Value> {
    let headers = github_headers(github_token, vs_code_version);
    let url = match account_type {
        "individual" => "https://api.github.com/copilot_internal/user".to_string(),
        _ => "https://api.github.com/copilot_internal/user".to_string(),
    };
    let mut request = client.get(&url);
    for (key, value) in &headers {
        request = request.header(key, value);
    }
    let response = request.send().await?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("Failed to get usage ({}): {}", status, text);
    }
    let data = response.json::<serde_json::Value>().await?;
    Ok(data)
}
