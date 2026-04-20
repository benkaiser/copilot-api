#[allow(dead_code)]
pub const COPILOT_VERSION: &str = "0.26.7";
pub const EDITOR_PLUGIN_VERSION: &str = "copilot-chat/0.26.7";
pub const USER_AGENT: &str = "GitHubCopilotChat/0.26.7";
pub const GITHUB_API_VERSION: &str = "2022-11-28";
pub const GITHUB_CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";
#[allow(dead_code)]
pub const GITHUB_APP_SCOPES: &str = "read:user";
pub const VSCODE_VERSION_FALLBACK: &str = "1.104.3";

pub fn copilot_base_url(account_type: &str) -> String {
    match account_type {
        "individual" => "https://api.githubcopilot.com".to_string(),
        other => format!("https://api.{}.githubcopilot.com", other),
    }
}

pub fn copilot_headers(
    copilot_token: &str,
    vs_code_version: &str,
    vision: bool,
) -> Vec<(String, String)> {
    let mut headers = vec![
        ("Authorization".to_string(), format!("Bearer {}", copilot_token)),
        ("content-type".to_string(), "application/json".to_string()),
        ("copilot-integration-id".to_string(), "vscode-chat".to_string()),
        ("editor-version".to_string(), format!("vscode/{}", vs_code_version)),
        ("editor-plugin-version".to_string(), EDITOR_PLUGIN_VERSION.to_string()),
        ("user-agent".to_string(), USER_AGENT.to_string()),
        ("openai-intent".to_string(), "conversation-panel".to_string()),
        ("x-request-id".to_string(), uuid::Uuid::new_v4().to_string()),
        (
            "x-vscode-user-agent-library-version".to_string(),
            "electron-fetch".to_string(),
        ),
    ];

    if vision {
        headers.push(("copilot-vision-request".to_string(), "true".to_string()));
    }

    headers
}

pub fn github_headers(github_token: &str, vs_code_version: &str) -> Vec<(String, String)> {
    vec![
        ("content-type".to_string(), "application/json".to_string()),
        ("accept".to_string(), "application/json".to_string()),
        ("authorization".to_string(), format!("token {}", github_token)),
        ("editor-version".to_string(), format!("vscode/{}", vs_code_version)),
        ("editor-plugin-version".to_string(), EDITOR_PLUGIN_VERSION.to_string()),
        ("user-agent".to_string(), USER_AGENT.to_string()),
        ("x-github-api-version".to_string(), GITHUB_API_VERSION.to_string()),
        (
            "x-vscode-user-agent-library-version".to_string(),
            "electron-fetch".to_string(),
        ),
    ]
}
