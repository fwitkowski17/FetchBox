//! dlmanager server binary: wires storage, API and the embedded frontend.

mod manager;
#[cfg(test)]
mod tests;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use axum::{
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
    Router,
};
use clap::Parser;
use dlm_api::auth::MIN_PASSWORD_LEN;
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
    /// Read the login password from this file (e.g. a Docker secret).
    /// Without it the `DLM_PASSWORD` environment variable is used.
    #[arg(long)]
    password_file: Option<PathBuf>,
    /// Print the OpenAPI document to stdout and exit (used to generate the frontend types).
    #[arg(long)]
    print_openapi: bool,
    /// Refuse downloads from private, loopback and link-local addresses (SSRF protection).
    /// Enable it when the manager is reachable from outside your network. Off by default,
    /// because fetching from a NAS or another host in the LAN is a normal use.
    #[arg(long)]
    block_private_urls: bool,
    /// Run without authentication. Anyone who can reach the port can use the manager.
    #[arg(long)]
    no_auth: bool,
}

/// Decide how the API is protected. Open access needs an explicit `--no-auth`, so a
/// forgotten password never silently exposes the manager. Without an external password
/// the account is kept in `store`: created in the browser on first run, guarded by a
/// one-time code written to the log.
fn resolve_auth(
    args: &Args,
    store: Arc<dyn dlm_api::auth::CredentialStore>,
) -> anyhow::Result<Arc<dlm_api::Auth>> {
    let from_file = match &args.password_file {
        Some(path) => Some(
            std::fs::read_to_string(path)
                .with_context(|| format!("read password file {}", path.display()))?,
        ),
        None => None,
    };
    // Trailing newlines are an artifact of how secrets files get written.
    let password = from_file
        .map(|p| p.trim_end_matches(['\r', '\n']).to_owned())
        .or_else(|| std::env::var("DLM_PASSWORD").ok())
        .filter(|p| !p.is_empty());

    match (password, args.no_auth) {
        (Some(_), true) => anyhow::bail!("--no-auth conflicts with a configured password"),
        (Some(p), false) => {
            anyhow::ensure!(
                p.chars().count() >= MIN_PASSWORD_LEN,
                "password must have at least {MIN_PASSWORD_LEN} characters"
            );
            if store
                .load_password_hash()
                .map_err(anyhow::Error::msg)?
                .is_some()
            {
                tracing::warn!(
                    "DLM_PASSWORD / --password-file takes precedence over the password set in the web UI"
                );
            }
            Ok(dlm_api::Auth::with_password(p))
        }
        (None, true) => {
            tracing::warn!("authentication is DISABLED (--no-auth)");
            Ok(dlm_api::Auth::disabled())
        }
        (None, false) => {
            let auth = dlm_api::Auth::from_store(store).map_err(anyhow::Error::msg)?;
            if let Some(code) = auth.setup_code() {
                tracing::warn!(
                    "first run: open the web UI and enter the setup code {code} to create your account \
                     (or set DLM_PASSWORD / --password-file)"
                );
            }
            Ok(auth)
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args = Args::parse();
    if args.print_openapi {
        // Before logging, passwords and directories: this must work on a bare checkout/CI.
        print!(
            "{}",
            dlm_api::openapi::spec_json().context("render OpenAPI")?
        );
        return Ok(());
    }
    std::fs::create_dir_all(&args.data_dir).context("create data dir")?;
    std::fs::create_dir_all(&args.download_dir).context("create download dir")?;

    let db_path = args.data_dir.join("dlmanager.db");
    let db = dlm_storage::open(&db_path).context("open database")?;
    tracing::info!(db = %db_path.display(), download_dir = %args.download_dir.display(), "storage ready");

    let manager = manager::Manager::start(
        db,
        args.download_dir.clone(),
        dlm_http_engine::UrlPolicy {
            block_private: args.block_private_urls,
        },
    )
    .await
    .context("start download manager")?;
    let auth = resolve_auth(&args, manager.clone())?;
    let app: Router = dlm_api::router(manager, auth).fallback(static_handler);

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

#[cfg(test)]
mod auth_config_tests {
    use super::*;

    fn args(password_file: Option<PathBuf>, no_auth: bool) -> Args {
        Args {
            data_dir: "d".into(),
            download_dir: "dl".into(),
            port: 0,
            password_file,
            print_openapi: false,
            block_private_urls: false,
            no_auth,
        }
    }

    /// Credential store backed by a value in memory.
    #[derive(Default)]
    struct MemStore(std::sync::Mutex<Option<String>>);

    impl dlm_api::auth::CredentialStore for MemStore {
        fn load_password_hash(&self) -> Result<Option<String>, String> {
            Ok(self.0.lock().expect("lock").clone())
        }
        fn save_password_hash(&self, hash: &str) -> Result<(), String> {
            *self.0.lock().expect("lock") = Some(hash.to_owned());
            Ok(())
        }
    }

    fn store() -> Arc<MemStore> {
        Arc::new(MemStore::default())
    }

    // `DLM_PASSWORD` is process-global, so everything touching it lives in one test.
    #[test]
    fn password_sources_and_validation() {
        std::env::remove_var("DLM_PASSWORD");

        // Nothing configured: locked until the account is created in the browser
        // (never open), with a setup code to log.
        let a = resolve_auth(&args(None, false), store()).expect("setup mode");
        assert!(a.required() && a.setup_required());
        assert!(a.setup_code().is_some());
        assert!(!resolve_auth(&args(None, true), store())
            .expect("no-auth")
            .required());

        // File secret, trailing newline stripped, short passwords refused.
        let dir = tempfile::tempdir().expect("tmp");
        let f = dir.path().join("pw");
        std::fs::write(&f, "correct-horse\n").expect("write");
        let a = resolve_auth(&args(Some(f.clone()), false), store()).expect("file");
        assert!(a.required() && !a.setup_required());
        assert!(a.login("correct-horse").is_ok());
        std::fs::write(&f, "short").expect("write");
        assert!(resolve_auth(&args(Some(f.clone()), false), store()).is_err());
        assert!(resolve_auth(&args(Some(dir.path().join("missing")), false), store()).is_err());

        // Environment variable.
        std::env::set_var("DLM_PASSWORD", "from-the-env");
        let a = resolve_auth(&args(None, false), store()).expect("env");
        assert!(a.required() && !a.can_change_password());
        // A password and --no-auth together is a mistake, not a silent choice.
        assert!(resolve_auth(&args(None, true), store()).is_err());

        // The external password wins over one created in the UI earlier.
        let used = store();
        let hash = "$argon2id$irrelevant".to_owned();
        dlm_api::auth::CredentialStore::save_password_hash(used.as_ref(), &hash).expect("save");
        let a = resolve_auth(&args(None, false), used).expect("env wins");
        assert!(a.login("from-the-env").is_ok());
        std::env::remove_var("DLM_PASSWORD");
    }
}
