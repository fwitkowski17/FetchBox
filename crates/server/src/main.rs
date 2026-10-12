//! dlmanager server binary: wires storage, API and the embedded frontend.

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Context;
use axum::{
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
    Router,
};
use clap::Parser;
use rust_embed::RustEmbed;

/// Frontend build output, embedded at compile time.
#[derive(RustEmbed)]
#[folder = "../../web/dist"]
struct Assets;

/// Self-hosted download manager.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Args {
    /// Directory for the SQLite database.
    #[arg(long, default_value = "./data")]
    data_dir: PathBuf,
    /// Directory where downloaded files are stored.
    #[arg(long, default_value = "./downloads")]
    download_dir: PathBuf,
    /// TCP port to listen on.
    #[arg(long, default_value_t = 8080)]
    port: u16,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args = Args::parse();
    std::fs::create_dir_all(&args.data_dir).context("create data dir")?;
    std::fs::create_dir_all(&args.download_dir).context("create download dir")?;

    let db_path = args.data_dir.join("dlmanager.db");
    let _db = dlm_storage::open(&db_path).context("open database")?;
    tracing::info!(db = %db_path.display(), download_dir = %args.download_dir.display(), "storage ready");

    let app: Router = dlm_api::router().fallback(static_handler);

    let addr = SocketAddr::from(([0, 0, 0, 0], args.port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("bind {addr}"))?;
    tracing::info!(%addr, "listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serve")?;
    Ok(())
}

/// Serve embedded files; unknown non-API paths fall back to `index.html` (SPA routes).
async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path == "api" || path.starts_with("api/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let path = if path.is_empty() { "index.html" } else { path };

    if let Some(resp) = embedded(path) {
        return resp;
    }
    // Missing file with an extension is a real 404; extensionless paths are SPA routes.
    let last = path.rsplit('/').next().unwrap_or(path);
    if !last.contains('.') {
        if let Some(resp) = embedded("index.html") {
            return resp;
        }
    }
    StatusCode::NOT_FOUND.into_response()
}

fn embedded(path: &str) -> Option<Response> {
    let file = Assets::get(path)?;
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    Some(
        (
            [(header::CONTENT_TYPE, mime.to_string())],
            file.data.into_owned(),
        )
            .into_response(),
    )
}

async fn shutdown_signal() {
    if let Err(e) = tokio::signal::ctrl_c().await {
        tracing::warn!(error = %e, "ctrl-c handler failed");
    }
    tracing::info!("shutting down");
}
