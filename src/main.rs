use std::sync::Arc;

use anyhow::Result;
use axum::{
    Router, middleware,
    routing::{get, post},
};
use clap::Parser;
use tokio::sync::RwLock;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod auth;
mod config;
mod handlers;
mod metadata;
mod ui;

use config::Config;
use metadata::Metadata;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub metadata: Arc<RwLock<Metadata>>,
}

#[derive(Parser)]
#[command(about = "Lightweight asset server")]
struct Cli {
    #[arg(short, long, default_value = "config.toml")]
    config: std::path::PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cli = Cli::parse();
    let config = Config::load(&cli.config)?;
    tokio::fs::create_dir_all(&config.storage_path).await?;

    let tmp_dir = config.storage_path.join(".tmp");
    if let Err(err) = tokio::fs::remove_dir_all(&tmp_dir).await
        && err.kind() != std::io::ErrorKind::NotFound
    {
        return Err(err.into());
    }
    tokio::fs::create_dir_all(&tmp_dir).await?;

    let metadata = Metadata::load(&config.storage_path).await?;
    tracing::info!(projects = metadata.projects.len(), "loaded metadata");

    let bind_addr = config.bind_addr.clone();
    let state = AppState {
        config: Arc::new(config),
        metadata: Arc::new(RwLock::new(metadata)),
    };

    let app = Router::new()
        .route("/", get(ui::index))
        .route("/ui/:project", get(ui::project))
        .route("/assets", post(handlers::create_project))
        .route("/assets/", post(handlers::create_project))
        .route(
            "/assets/:project",
            post(handlers::upload)
                .get(handlers::metadata)
                .put(handlers::update_project),
        )
        .route("/assets/:project/:filename", get(handlers::download))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ))
        .with_state(state);

    tracing::info!("listening on http://{bind_addr}");
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
