use clap::{Parser, Subcommand};
use std::sync::Arc;

mod anthropic;
mod auth;
mod config;
mod error;
mod openai_types;
mod paths;
mod proxy;
mod rate_limit;
mod server;
mod state;
mod vscode_version;

use auth::token;
use state::AppState;

#[derive(Parser)]
#[command(name = "copilot-api", about = "Copilot API proxy server")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the Copilot API proxy server
    Start {
        /// Port to listen on
        #[arg(short, long, default_value = "4141")]
        port: u16,

        /// Enable verbose logging
        #[arg(short, long)]
        verbose: bool,

        /// Account type (individual, business, enterprise)
        #[arg(short, long, default_value = "individual")]
        account_type: String,

        /// Enable manual request approval
        #[arg(long)]
        manual: bool,

        /// Rate limit in seconds between requests
        #[arg(short, long)]
        rate_limit: Option<u64>,

        /// Wait instead of error when rate limit is hit
        #[arg(short, long)]
        wait: bool,

        /// Provide GitHub token directly
        #[arg(short, long)]
        github_token: Option<String>,

        /// Show GitHub and Copilot tokens
        #[arg(long)]
        show_token: bool,
    },
    /// Run GitHub authentication flow
    Auth {
        /// Enable verbose logging
        #[arg(short, long)]
        verbose: bool,

        /// Show GitHub token on auth
        #[arg(long)]
        show_token: bool,
    },
    /// Show Copilot usage and quota information
    CheckUsage {
        /// Enable verbose logging
        #[arg(short, long)]
        verbose: bool,
    },
    /// Display diagnostic information
    Debug {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Start {
            port,
            verbose,
            account_type,
            manual,
            rate_limit,
            wait,
            github_token,
            show_token,
        } => {
            init_logging(verbose);
            run_start(
                port,
                account_type,
                manual,
                rate_limit,
                wait,
                github_token,
                show_token,
            )
            .await?;
        }
        Commands::Auth { verbose, show_token } => {
            init_logging(verbose);
            run_auth(show_token).await?;
        }
        Commands::CheckUsage { verbose } => {
            init_logging(verbose);
            run_check_usage().await?;
        }
        Commands::Debug { json } => {
            run_debug(json).await?;
        }
    }

    Ok(())
}

fn init_logging(verbose: bool) {
    let filter = if verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(filter)),
        )
        .init();
}

async fn run_start(
    port: u16,
    account_type: String,
    manual: bool,
    rate_limit: Option<u64>,
    wait: bool,
    github_token: Option<String>,
    show_token: bool,
) -> anyhow::Result<()> {
    // Ensure paths exist
    paths::ensure_paths()?;

    // Create state
    let mut app_state = AppState::new(account_type);
    app_state.rate_limit_seconds = rate_limit;
    app_state.rate_limit_wait = wait;
    app_state.show_token = show_token;
    app_state.manual_approve = manual;

    let state = Arc::new(app_state);

    // Cache VSCode version
    tracing::info!("Fetching VSCode version...");
    let version = vscode_version::fetch_vscode_version(&state.http_client).await;
    *state.vs_code_version.write().await = Some(version.clone());
    tracing::info!("Using VSCode version: {}", version);

    // Set up GitHub token
    if let Some(token) = github_token {
        *state.github_token.write().await = Some(token);
    } else {
        token::setup_github_token(&state, false).await?;
    }

    // Set up Copilot token
    token::setup_copilot_token(&state).await?;

    // Cache models
    tracing::info!("Fetching available models...");
    cache_models(&state).await?;

    // Create router and start server
    let app = server::create_router(state);
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    tracing::info!("Copilot API proxy listening on http://0.0.0.0:{}", port);

    axum::serve(listener, app).await?;

    Ok(())
}

async fn cache_models(state: &Arc<AppState>) -> anyhow::Result<()> {
    let copilot_token = state
        .copilot_token
        .read()
        .await
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Copilot token not set"))?;
    let vs_code_version = state
        .vs_code_version
        .read()
        .await
        .clone()
        .unwrap_or_else(|| config::VSCODE_VERSION_FALLBACK.to_string());

    let base_url = config::copilot_base_url(&state.account_type);
    let url = format!("{}/models", base_url);
    let headers = config::copilot_headers(&copilot_token, &vs_code_version, false);

    let mut request = state.http_client.get(&url);
    for (key, value) in &headers {
        request = request.header(key, value);
    }

    let response = request.send().await?;
    if response.status().is_success() {
        let models: openai_types::ModelsResponse = response.json().await?;
        tracing::info!("Loaded {} models", models.data.len());
        *state.models.write().await = Some(models);
    } else {
        tracing::warn!("Failed to fetch models: {}", response.status());
    }

    Ok(())
}

async fn run_auth(show_token: bool) -> anyhow::Result<()> {
    paths::ensure_paths()?;

    let mut app_state = AppState::new("individual".to_string());
    app_state.show_token = show_token;
    let state = Arc::new(app_state);

    let version = vscode_version::fetch_vscode_version(&state.http_client).await;
    *state.vs_code_version.write().await = Some(version);

    token::setup_github_token(&state, true).await?;

    tracing::info!("Authentication complete!");
    Ok(())
}

async fn run_check_usage() -> anyhow::Result<()> {
    paths::ensure_paths()?;

    let state = Arc::new(AppState::new("individual".to_string()));
    let version = vscode_version::fetch_vscode_version(&state.http_client).await;
    *state.vs_code_version.write().await = Some(version.clone());

    // Read existing token
    let token_path = paths::github_token_path();
    let github_token = std::fs::read_to_string(&token_path)?
        .trim()
        .to_string();

    if github_token.is_empty() {
        anyhow::bail!("No GitHub token found. Run 'copilot-api auth' first.");
    }

    let usage = auth::github::get_copilot_usage(
        &state.http_client,
        &github_token,
        &version,
        "individual",
    )
    .await?;

    println!("{}", serde_json::to_string_pretty(&usage)?);
    Ok(())
}

async fn run_debug(json: bool) -> anyhow::Result<()> {
    let token_path = paths::github_token_path();
    let has_token = std::fs::read_to_string(&token_path)
        .map(|t| !t.trim().is_empty())
        .unwrap_or(false);

    let info = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "runtime": "rust",
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "app_dir": paths::app_dir().display().to_string(),
        "token_path": token_path.display().to_string(),
        "has_github_token": has_token,
    });

    if json {
        println!("{}", serde_json::to_string_pretty(&info)?);
    } else {
        println!("Copilot API Debug Info");
        println!("──────────────────────");
        println!("Version:          {}", env!("CARGO_PKG_VERSION"));
        println!("Runtime:          Rust");
        println!("Platform:         {}", std::env::consts::OS);
        println!("Arch:             {}", std::env::consts::ARCH);
        println!("App Dir:          {}", paths::app_dir().display());
        println!("Token Path:       {}", token_path.display());
        println!("Has GitHub Token: {}", has_token);
    }

    Ok(())
}
