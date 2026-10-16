//! FetchBox server binary: wires storage, API and the embedded frontend.

mod assets;
mod clock;
mod logfmt;
mod manager;
#[cfg(test)]
mod tests;
mod update;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use axum::Router;
use clap::Parser;
use fetchbox_api::auth::MIN_PASSWORD_LEN;

/// Self-hosted download manager.
#[derive(Debug, Parser)]
#[command(name = "fetchbox", version, about)]
struct Args {
    /// Directory for the SQLite database.
    #[arg(long, default_value = "./data")]
    data_dir: PathBuf,
    /// Directory where downloaded files are stored.
    #[arg(long, default_value = "./downloads")]
    download_dir: PathBuf,
    /// Log what happens to downloads and settings (added, paused, finished, removed with the
    /// average speed, settings changes, download hours, sign-ins). Warnings and errors are always logged.
    #[arg(long, env = "FETCHBOX_LOG_ACTIVITY")]
    log_activity: bool,
    /// TCP port to listen on.
    #[arg(long, default_value_t = 8080)]
    port: u16,
    /// Read the login password from this file (e.g. a Docker secret).
    /// Without it the `FETCHBOX_PASSWORD` environment variable is used.
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
    #[command(subcommand)]
    command: Option<Command>,
}

/// The password from the environment: `FETCHBOX_PASSWORD`, or the variable's name from before the
/// project was renamed, `DLM_PASSWORD` (still honoured, so an existing deployment keeps working).
///
/// An empty value counts as not set: `docker compose` passes `FETCHBOX_PASSWORD: ${FETCHBOX_PASSWORD:-}`.
fn password_from_env() -> Option<String> {
    let set = |name: &str| std::env::var(name).ok().filter(|p| !p.is_empty());
    if let Some(p) = set("FETCHBOX_PASSWORD") {
        return Some(p);
    }
    let old = set("DLM_PASSWORD")?;
    tracing::warn!("DLM_PASSWORD is the old name of FETCHBOX_PASSWORD; please rename it");
    Some(old)
}

/// Before the rename the database was `dlmanager.db`. If only that exists, move it (and its
/// SQLite side files) to the new name so nobody loses their downloads or their account.
fn adopt_legacy_database(dir: &Path) -> anyhow::Result<()> {
    let old = dir.join("dlmanager.db");
    let new = dir.join("fetchbox.db");
    if new.exists() || !old.exists() {
        return Ok(());
    }
    let named = |base: &Path, suffix: &str| {
        let mut name = base.as_os_str().to_owned();
        name.push(suffix);
        PathBuf::from(name)
    };
    // Side files first, the database last: if this is interrupted the next start finds the old
    // database again and the side files already carry the new name they will be paired with.
    for suffix in ["-wal", "-shm", "-journal", ""] {
        let from = named(&old, suffix);
        if from.exists() {
            std::fs::rename(&from, named(&new, suffix))
                .with_context(|| format!("rename {}", from.display()))?;
        }
    }
    tracing::info!(from = %old.display(), to = %new.display(), "renamed the database after the project rename");
    Ok(())
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Forget the password created in the web UI (for a forgotten password). Stop the server first,
    /// run this, start the server again: it asks for a new account like on the first run and
    /// prints the setup code in its log. Downloads and settings are kept.
    ResetPassword,
}

/// `reset-password`: remove the stored password hash from the database in `data_dir`.
fn reset_password(data_dir: &Path) -> anyhow::Result<()> {
    adopt_legacy_database(data_dir)?;
    let db_path = data_dir.join("fetchbox.db");
    // Never create a database here: a wrong --data-dir would "succeed" on an empty one.
    anyhow::ensure!(
        db_path.exists(),
        "no database in {}: is --data-dir the directory the server uses?",
        data_dir.display()
    );
    let db = fetchbox_storage::open(&db_path).context("open database")?;
    if fetchbox_storage::settings::clear_password_hash(&db).context("remove the password")? {
        tracing::info!(
            "password removed. Start the server again: it will ask for a new account, with a setup code in its log"
        );
    } else {
        tracing::info!("no password is stored in the web UI: nothing to reset");
    }
    if password_from_env().is_some() {
        tracing::warn!(
            "FETCHBOX_PASSWORD is set here: a password from the environment or from --password-file always wins; change or remove it there"
        );
    }
    Ok(())
}

/// Decide how the API is protected. Open access needs an explicit `--no-auth`, so a
/// forgotten password never silently exposes the manager. Without an external password
/// the account is kept in `store`: created in the browser on first run, guarded by a
/// one-time code written to the log.
fn resolve_auth(
    args: &Args,
    store: Arc<dyn fetchbox_api::auth::CredentialStore>,
) -> anyhow::Result<Arc<fetchbox_api::Auth>> {
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
        .or_else(password_from_env)
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
                    "FETCHBOX_PASSWORD / --password-file takes precedence over the password set in the web UI"
                );
            }
            Ok(fetchbox_api::Auth::with_password(p))
        }
        (None, true) => {
            tracing::warn!("authentication is DISABLED (--no-auth)");
            Ok(fetchbox_api::Auth::disabled())
        }
        (None, false) => {
            let auth = fetchbox_api::Auth::from_store(store).map_err(anyhow::Error::msg)?;
            if let Some(code) = auth.setup_code() {
                tracing::warn!(
                    "first run: open the web UI and enter the setup code {code} to create your account \
                     (or set FETCHBOX_PASSWORD / --password-file)"
                );
            }
            Ok(auth)
        }
    }
}

/// How long open connections get to finish after the shutdown signal before they are dropped.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);
/// How long the runtime waits for blocking work (a name lookup stuck in the OS, a slow disk) when
/// the process exits. It would wait for ever, and the server would never quit.
const RUNTIME_EXIT_WAIT: Duration = Duration::from_secs(3);

fn main() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("start the async runtime")?;
    let result = runtime.block_on(run());
    runtime.shutdown_timeout(RUNTIME_EXIT_WAIT);
    result
}

/// Console logging. The per-event lines (target `activity`) are shown only with `--log-activity`;
/// `RUST_LOG` still sets the level of everything else.
fn init_logging(activity: bool) {
    let mut filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let level = if activity { "info" } else { "warn" };
    if let Ok(directive) = format!("activity={level}").parse() {
        filter = filter.add_directive(directive);
    }
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

async fn run() -> anyhow::Result<()> {
    let args = Args::parse();
    init_logging(args.log_activity);
    if args.print_openapi {
        // Before logging, passwords and directories: this must work on a bare checkout/CI.
        print!(
            "{}",
            fetchbox_api::openapi::spec_json().context("render OpenAPI")?
        );
        return Ok(());
    }
    if let Some(Command::ResetPassword) = args.command {
        return reset_password(&args.data_dir);
    }
    std::fs::create_dir_all(&args.data_dir).context("create data dir")?;
    std::fs::create_dir_all(&args.download_dir).context("create download dir")?;

    adopt_legacy_database(&args.data_dir)?;
    let db_path = args.data_dir.join("fetchbox.db");
    let db = fetchbox_storage::open(&db_path).context("open database")?;
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "FetchBox starting");
    tracing::info!(db = %db_path.display(), download_dir = %args.download_dir.display(), "storage ready");

    let manager = manager::Manager::start(
        db,
        args.download_dir.clone(),
        fetchbox_http_engine::UrlPolicy {
            block_private: args.block_private_urls,
        },
    )
    .await
    .context("start download manager")?;
    let auth = resolve_auth(&args, manager.clone())?;
    let app: Router = fetchbox_api::router(manager.clone(), auth).fallback(assets::handler);

    let addr = SocketAddr::from(([0, 0, 0, 0], args.port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("bind {addr}"))?;
    tracing::info!(%addr, "listening");

    // Graceful: stop accepting, let requests finish. The event streams never finish by themselves,
    // so they are told to end; whatever still hangs on after the grace period is dropped.
    let mut closing = manager.closing_receiver();
    let serve = axum::serve(listener, app).with_graceful_shutdown({
        let manager = manager.clone();
        async move {
            shutdown_signal().await;
            manager.shutdown();
            // A second signal means "now": do not make anybody reach for `kill -9`.
            tokio::spawn(async {
                shutdown_signal().await;
                tracing::warn!("second signal, exiting at once");
                std::process::exit(130);
            });
        }
    });
    tokio::select! {
        served = serve => served.context("serve")?,
        () = async {
            _ = closing.wait_for(|c| *c).await;
            tokio::time::sleep(SHUTDOWN_GRACE).await;
        } => tracing::warn!("closing connections that did not finish in time"),
    }
    tracing::info!("stopped");
    Ok(())
}

/// Resolves on Ctrl-C and, on Unix, on SIGTERM (`kill`, `docker stop`, launchd, systemd).
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::warn!(error = %e, "ctrl-c handler failed");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let terminate = async {
            match signal(SignalKind::terminate()) {
                Ok(mut s) => {
                    s.recv().await;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "SIGTERM handler failed");
                    std::future::pending::<()>().await;
                }
            }
        };
        tokio::select! { () = ctrl_c => {}, () = terminate => {} }
    }
    #[cfg(not(unix))]
    ctrl_c.await;
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
            log_activity: false,
            no_auth,
            command: None,
        }
    }

    /// Credential store backed by a value in memory.
    #[derive(Default)]
    struct MemStore(std::sync::Mutex<Option<String>>);

    impl fetchbox_api::auth::CredentialStore for MemStore {
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

    // `FETCHBOX_PASSWORD` is process-global, so everything touching it lives in one test.
    #[test]
    fn password_sources_and_validation() {
        std::env::remove_var("FETCHBOX_PASSWORD");

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
        std::env::set_var("FETCHBOX_PASSWORD", "from-the-env");
        let a = resolve_auth(&args(None, false), store()).expect("env");
        assert!(a.required() && !a.can_change_password());
        // A password and --no-auth together is a mistake, not a silent choice.
        assert!(resolve_auth(&args(None, true), store()).is_err());

        // The external password wins over one created in the UI earlier.
        let used = store();
        let hash = "$argon2id$irrelevant".to_owned();
        fetchbox_api::auth::CredentialStore::save_password_hash(used.as_ref(), &hash)
            .expect("save");
        let a = resolve_auth(&args(None, false), used).expect("env wins");
        assert!(a.login("from-the-env").is_ok());
        std::env::remove_var("FETCHBOX_PASSWORD");

        // The old name of the variable still works, and the new one wins when both are set.
        std::env::set_var("DLM_PASSWORD", "from-the-old-name");
        let a = resolve_auth(&args(None, false), store()).expect("old name");
        assert!(a.login("from-the-old-name").is_ok());
        std::env::set_var("FETCHBOX_PASSWORD", "from-the-new-name");
        let a = resolve_auth(&args(None, false), store()).expect("both");
        assert!(a.login("from-the-new-name").is_ok());
        assert!(a.login("from-the-old-name").is_err());
        std::env::remove_var("FETCHBOX_PASSWORD");
        std::env::remove_var("DLM_PASSWORD");
    }
}

#[cfg(test)]
mod rename_tests {
    use super::*;

    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .expect("dir")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn the_old_database_is_moved_with_its_side_files() {
        let dir = tempfile::tempdir().expect("tmp");
        for (name, body) in [
            ("dlmanager.db", "db"),
            ("dlmanager.db-wal", "wal"),
            ("dlmanager.db-shm", "shm"),
        ] {
            std::fs::write(dir.path().join(name), body).expect("write");
        }
        adopt_legacy_database(dir.path()).expect("adopt");
        assert_eq!(
            files(dir.path()),
            ["fetchbox.db", "fetchbox.db-shm", "fetchbox.db-wal"]
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("fetchbox.db")).expect("read"),
            "db"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("fetchbox.db-wal")).expect("read"),
            "wal"
        );
    }

    #[test]
    fn an_existing_new_database_is_never_replaced() {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::write(dir.path().join("dlmanager.db"), "old").expect("write");
        std::fs::write(dir.path().join("fetchbox.db"), "new").expect("write");
        adopt_legacy_database(dir.path()).expect("adopt");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("fetchbox.db")).expect("read"),
            "new"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("dlmanager.db")).expect("read"),
            "old"
        );
    }

    #[test]
    fn nothing_to_do_on_a_fresh_install() {
        let dir = tempfile::tempdir().expect("tmp");
        adopt_legacy_database(dir.path()).expect("adopt");
        assert!(files(dir.path()).is_empty());
    }

    #[test]
    fn an_interrupted_move_is_finished_next_time() {
        // The side files were moved already, the database was not.
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::write(dir.path().join("dlmanager.db"), "db").expect("write");
        std::fs::write(dir.path().join("fetchbox.db-wal"), "wal").expect("write");
        adopt_legacy_database(dir.path()).expect("adopt");
        assert_eq!(files(dir.path()), ["fetchbox.db", "fetchbox.db-wal"]);
    }
}
