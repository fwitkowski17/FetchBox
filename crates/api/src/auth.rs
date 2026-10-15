//! Single-user authentication (SPEC 6.7).
//!
//! One shared password. A successful login creates a random session token, delivered
//! as an `HttpOnly` cookie (needed for SSE, `EventSource` cannot set headers) and in the
//! JSON body for API clients that send `Authorization: Bearer <token>`.
//!
//! Where the password comes from decides what the UI may do with it:
//! - `--no-auth`: nothing is checked.
//! - `DLM_PASSWORD` / `--password-file`: an external secret; it wins over everything and
//!   cannot be changed from the UI.
//! - otherwise the account is created in the browser on first run (`POST /auth/setup`),
//!   guarded by a one-time setup code printed to the server log, so whoever reaches a fresh
//!   server first cannot take it over. The password is stored as an Argon2id hash.
//!
//! Sessions live in memory: a restart logs everybody out, which is fine for one user and
//! avoids persisting secrets. Failed logins are throttled globally (one user, and no
//! trusting of client IPs behind a reverse proxy).

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::ErrorBody;

const COOKIE: &str = "dlm_session";
const SESSION_TTL: Duration = Duration::from_secs(7 * 24 * 3600);
const MAX_SESSIONS: usize = 100;
const MAX_FAILURES: usize = 5;
const FAILURE_WINDOW: Duration = Duration::from_secs(60);

/// Shorter passwords are refused (startup, setup and password change alike).
pub const MIN_PASSWORD_LEN: usize = 8;
/// Upper bound so a request cannot make the server hash megabytes.
const MAX_PASSWORD_LEN: usize = 256;
/// Setup code alphabet: no `0/O/1/I/L`, easy to read from a log and type on a phone.
const CODE_ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";
const CODE_LEN: usize = 8;

/// Why a login did not succeed.
#[derive(Debug, PartialEq, Eq)]
pub enum LoginError {
    /// Wrong password.
    Invalid,
    /// Too many recent failures; retry after this many seconds.
    Locked(u64),
    /// The system could not produce a secure token (no OS randomness).
    Unavailable,
}

/// Why the first-run setup or a password change was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum AccountError {
    /// Wrong setup code or wrong current password.
    Invalid,
    /// Too many recent failures; retry after this many seconds.
    Locked(u64),
    /// The new password is shorter than [`MIN_PASSWORD_LEN`] or absurdly long.
    BadPassword,
    /// Not allowed in the current mode (setup already done, or the password is external).
    Conflict,
    /// Randomness, hashing or storage failed.
    Internal,
}

/// Persistence for the password hash created through the UI (implemented over SQLite by
/// the server, so this crate stays free of SQL).
pub trait CredentialStore: Send + Sync {
    fn load_password_hash(&self) -> Result<Option<String>, String>;
    fn save_password_hash(&self, hash: &str) -> Result<(), String>;
}

/// How requests are authenticated right now.
enum Mode {
    /// `--no-auth`.
    Disabled,
    /// Secret from the environment or a file; compared directly, never changeable here.
    External(String),
    /// Argon2id hash (PHC string) created through the UI.
    Stored(String),
    /// Nobody has an account yet; this code authorises creating it.
    Setup { code: String },
}

/// Authentication state shared by all requests.
pub struct Auth {
    mode: Mutex<Mode>,
    store: Option<Arc<dyn CredentialStore>>,
    ttl: Duration,
    sessions: Mutex<HashMap<String, Instant>>,
    failures: Mutex<VecDeque<Instant>>,
}

impl Auth {
    /// No authentication: every request is allowed (explicit `--no-auth`).
    pub fn disabled() -> Arc<Self> {
        Arc::new(Self::new(Mode::Disabled, None, SESSION_TTL))
    }

    /// Require `password` (from the environment or a file) for everything except health
    /// and the auth endpoints.
    pub fn with_password(password: impl Into<String>) -> Arc<Self> {
        Arc::new(Self::new(
            Mode::External(password.into()),
            None,
            SESSION_TTL,
        ))
    }

    /// Account kept in `store`: use the stored hash, or start in setup mode (with a fresh
    /// one-time code, see [`Auth::setup_code`]) when there is none yet.
    pub fn from_store(store: Arc<dyn CredentialStore>) -> Result<Arc<Self>, String> {
        let mode = match store.load_password_hash()? {
            Some(hash) => Mode::Stored(hash),
            None => Mode::Setup {
                code: new_setup_code().ok_or("no OS randomness for the setup code")?,
            },
        };
        Ok(Arc::new(Self::new(mode, Some(store), SESSION_TTL)))
    }

    fn new(mode: Mode, store: Option<Arc<dyn CredentialStore>>, ttl: Duration) -> Self {
        Self {
            mode: Mutex::new(mode),
            store,
            ttl,
            sessions: Mutex::new(HashMap::new()),
            failures: Mutex::new(VecDeque::new()),
        }
    }

    /// Whether requests must be authenticated (also true while the account is not set up:
    /// everything is locked until it is).
    pub fn required(&self) -> bool {
        !matches!(*lock(&self.mode), Mode::Disabled)
    }

    /// The account has not been created yet.
    pub fn setup_required(&self) -> bool {
        matches!(*lock(&self.mode), Mode::Setup { .. })
    }

    /// The one-time setup code, while setup is pending (for the startup log).
    pub fn setup_code(&self) -> Option<String> {
        match &*lock(&self.mode) {
            Mode::Setup { code } => Some(code.clone()),
            _ => None,
        }
    }

    /// The password can be changed through the API (created in the UI, not external).
    pub fn can_change_password(&self) -> bool {
        matches!(*lock(&self.mode), Mode::Stored(_))
    }

    /// Reserve an attempt: fail while locked. Failures are recorded by [`Auth::fail`].
    fn check_throttle(&self) -> Result<(), u64> {
        let now = Instant::now();
        let mut failures = lock(&self.failures);
        while failures
            .front()
            .is_some_and(|t| now.duration_since(*t) >= FAILURE_WINDOW)
        {
            failures.pop_front();
        }
        if failures.len() >= MAX_FAILURES {
            let oldest = failures.front().copied().unwrap_or(now);
            let wait = FAILURE_WINDOW.saturating_sub(now.duration_since(oldest));
            return Err(wait.as_secs().max(1));
        }
        Ok(())
    }

    fn fail(&self) {
        lock(&self.failures).push_back(Instant::now());
    }

    fn clear_failures(&self) {
        lock(&self.failures).clear();
    }

    /// Open a session; returns its token. `None` without OS randomness.
    fn open_session(&self) -> Option<String> {
        let token = new_token()?;
        let now = Instant::now();
        let mut sessions = lock(&self.sessions);
        sessions.retain(|_, expires| *expires > now);
        if sessions.len() >= MAX_SESSIONS {
            // Drop the session closest to expiry to keep memory bounded.
            if let Some(oldest) = sessions
                .iter()
                .min_by_key(|(_, e)| **e)
                .map(|(k, _)| k.clone())
            {
                sessions.remove(&oldest);
            }
        }
        sessions.insert(token.clone(), now + self.ttl);
        Some(token)
    }

    /// Check the password and open a session; returns its token. Hashing is slow on
    /// purpose: call from `spawn_blocking` in async code.
    pub fn login(&self, attempt: &str) -> Result<String, LoginError> {
        // Snapshot, so the (slow) hash check does not hold the lock every request takes.
        enum Check {
            Password(String),
            Hash(String),
        }
        let check = match &*lock(&self.mode) {
            Mode::Disabled => return Ok(String::new()),
            // Nobody can log in before the account exists.
            Mode::Setup { .. } => return Err(LoginError::Invalid),
            Mode::External(p) => Check::Password(p.clone()),
            Mode::Stored(h) => Check::Hash(h.clone()),
        };
        self.check_throttle().map_err(LoginError::Locked)?;
        let ok = match check {
            Check::Password(p) => constant_time_eq(p.as_bytes(), attempt.as_bytes()),
            Check::Hash(h) => verify_hash(&h, attempt),
        };
        if !ok {
            self.fail();
            return Err(LoginError::Invalid);
        }
        self.clear_failures();
        self.open_session().ok_or(LoginError::Unavailable)
    }

    /// Create the account: `code` must be the setup code, `password` the new password.
    /// Succeeds once; logs the caller in. Slow (hashing), see [`Auth::login`].
    pub fn setup(&self, code: &str, password: &str) -> Result<String, AccountError> {
        let mut mode = lock(&self.mode);
        let Mode::Setup { code: expected } = &*mode else {
            return Err(AccountError::Conflict);
        };
        self.check_throttle().map_err(AccountError::Locked)?;
        // Normalise what a person types from a log line: case and the cosmetic dash.
        let typed: String = code
            .chars()
            .filter(|c| *c != '-' && !c.is_whitespace())
            .map(|c| c.to_ascii_uppercase())
            .collect();
        if !constant_time_eq(expected.as_bytes(), typed.as_bytes()) {
            self.fail();
            return Err(AccountError::Invalid);
        }
        check_password(password)?;
        let hash = hash_password(password).ok_or(AccountError::Internal)?;
        let store = self.store.as_ref().ok_or(AccountError::Internal)?;
        store.save_password_hash(&hash).map_err(|e| {
            tracing::error!(error = %e, "cannot store the password hash");
            AccountError::Internal
        })?;
        *mode = Mode::Stored(hash);
        drop(mode);
        self.clear_failures();
        self.open_session().ok_or(AccountError::Internal)
    }

    /// Replace the password. Every other session is ended; the caller's (`keep`) survives.
    /// Slow (hashing), see [`Auth::login`].
    pub fn change_password(
        &self,
        keep: &str,
        current: &str,
        new: &str,
    ) -> Result<(), AccountError> {
        let mut mode = lock(&self.mode);
        let Mode::Stored(hash) = &*mode else {
            return Err(AccountError::Conflict);
        };
        self.check_throttle().map_err(AccountError::Locked)?;
        if !verify_hash(hash, current) {
            self.fail();
            return Err(AccountError::Invalid);
        }
        check_password(new)?;
        let new_hash = hash_password(new).ok_or(AccountError::Internal)?;
        let store = self.store.as_ref().ok_or(AccountError::Internal)?;
        store.save_password_hash(&new_hash).map_err(|e| {
            tracing::error!(error = %e, "cannot store the password hash");
            AccountError::Internal
        })?;
        *mode = Mode::Stored(new_hash);
        drop(mode);
        self.clear_failures();
        lock(&self.sessions).retain(|t, _| t == keep);
        Ok(())
    }

    /// Whether `token` belongs to a live session (always true when auth is off).
    pub fn is_valid(&self, token: Option<&str>) -> bool {
        if !self.required() {
            return true;
        }
        let Some(token) = token else { return false };
        let mut sessions = lock(&self.sessions);
        match sessions.get(token) {
            Some(expires) if *expires > Instant::now() => true,
            Some(_) => {
                sessions.remove(token);
                false
            }
            None => false,
        }
    }

    /// End a session; unknown tokens are ignored.
    pub fn logout(&self, token: &str) {
        lock(&self.sessions).remove(token);
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// 256 random bits, hex encoded. `None` when the OS cannot provide randomness: there is
/// no safe fallback for a secret, so login fails instead.
fn new_token() -> Option<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).ok()?;
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// A random setup code such as `K7QM2XPD`.
fn new_setup_code() -> Option<String> {
    let mut bytes = [0u8; CODE_LEN];
    getrandom::fill(&mut bytes).ok()?;
    // 256 is not a multiple of the alphabet size, so values are slightly biased; the code
    // is a throttled one-time gate, not a key, and 8 symbols still give about 39 bits.
    Some(
        bytes
            .iter()
            .map(|b| char::from(CODE_ALPHABET[usize::from(*b) % CODE_ALPHABET.len()]))
            .collect(),
    )
}

fn check_password(password: &str) -> Result<(), AccountError> {
    let n = password.chars().count();
    if (MIN_PASSWORD_LEN..=MAX_PASSWORD_LEN).contains(&n) {
        Ok(())
    } else {
        Err(AccountError::BadPassword)
    }
}

/// Argon2id (library defaults) with a random salt, as a PHC string.
fn hash_password(password: &str) -> Option<String> {
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).ok()?;
    let salt = SaltString::encode_b64(&salt).ok()?;
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .ok()?;
    Some(hash.to_string())
}

fn verify_hash(hash: &str, attempt: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|h| {
        Argon2::default()
            .verify_password(attempt.as_bytes(), &h)
            .is_ok()
    })
}

/// Compare without stopping at the first difference.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        diff |= usize::from(a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0));
    }
    diff == 0
}

/// Token from `Authorization: Bearer ...` or the session cookie.
fn token_from(headers: &HeaderMap) -> Option<String> {
    if let Some(v) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(t) = v.strip_prefix("Bearer ") {
            return Some(t.trim().to_owned());
        }
    }
    headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())?
        .split(';')
        .filter_map(|p| p.trim().split_once('='))
        .find(|(k, _)| *k == COOKIE)
        .map(|(_, v)| v.to_owned())
}

fn session_cookie(token: &str, max_age: u64, headers: &HeaderMap) -> HeaderValue {
    // `Secure` only when a reverse proxy says the browser talks HTTPS; on plain LAN HTTP
    // the browser would drop a Secure cookie and nobody could log in.
    let https = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("https"));
    let secure = if https { "; Secure" } else { "" };
    let value =
        format!("{COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={max_age}{secure}");
    // Token is hex and the rest is static, so this cannot fail; fall back to clearing.
    HeaderValue::from_str(&value).unwrap_or_else(|_| HeaderValue::from_static(""))
}

pub(crate) fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(ErrorBody {
            error: "unauthorized".into(),
        }),
    )
        .into_response()
}

/// Middleware for everything that needs a logged-in user.
pub async fn require_auth(State(auth): State<Arc<Auth>>, req: Request, next: Next) -> Response {
    if auth.is_valid(token_from(req.headers()).as_deref()) {
        next.run(req).await
    } else {
        unauthorized()
    }
}

/// `/auth/*` routes (public).
pub fn router(auth: Arc<Auth>) -> Router {
    Router::new()
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/status", get(status))
        .route("/auth/setup", post(setup))
        .route("/auth/password", post(change_password))
        .with_state(auth)
}

/// Login request.
#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginRequest {
    pub password: String,
}

/// Successful login. The same token is also set as the `dlm_session` cookie.
#[derive(Debug, Serialize, ToSchema)]
pub struct LoginResponse {
    /// Session token, usable as `Authorization: Bearer <token>`. Empty when auth is disabled.
    pub token: String,
}

/// Too many failed logins.
#[derive(Debug, Serialize, ToSchema)]
pub struct LockedBody {
    pub error: String,
    /// Seconds until the next attempt is allowed.
    pub retry_after: u64,
}

/// What the UI needs to decide whether to show the login screen.
#[derive(Debug, Serialize, ToSchema)]
pub struct AuthStatus {
    /// A password is configured; unauthenticated requests are refused.
    pub auth_required: bool,
    /// The caller's session (or the disabled auth) lets it through.
    pub authenticated: bool,
    /// No account exists yet: show the first-run setup (`POST /api/auth/setup`).
    pub setup_required: bool,
    /// The password was created in the UI and can be changed (`POST /api/auth/password`);
    /// false for a password from `DLM_PASSWORD` / `--password-file`.
    pub can_change_password: bool,
}

/// First-run account creation.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SetupRequest {
    /// One-time code printed in the server log at startup (case and dashes are ignored).
    pub code: String,
    /// The new password, at least 8 characters.
    pub password: String,
}

/// Password change.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    /// At least 8 characters.
    pub new_password: String,
}

/// Log in and start a session.
#[utoipa::path(
    post,
    path = "/api/auth/login",
    tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Logged in; sets the `dlm_session` cookie", body = LoginResponse),
        (status = 401, description = "Wrong password", body = ErrorBody),
        (status = 429, description = "Too many failed attempts (5 per minute); see `Retry-After`", body = LockedBody),
    )
)]
pub(crate) async fn login(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> Response {
    let attempt = body.password;
    let worker = Arc::clone(&auth);
    // Argon2 is CPU and memory heavy: keep it off the async workers.
    let result = tokio::task::spawn_blocking(move || worker.login(&attempt))
        .await
        .unwrap_or(Err(LoginError::Unavailable));
    match result {
        Ok(token) => {
            let mut resp = Json(LoginResponse {
                token: token.clone(),
            })
            .into_response();
            if auth.required() {
                resp.headers_mut().insert(
                    header::SET_COOKIE,
                    session_cookie(&token, auth.ttl.as_secs(), &headers),
                );
            }
            resp
        }
        Err(LoginError::Invalid) => {
            tracing::warn!("failed login attempt");
            unauthorized()
        }
        Err(LoginError::Unavailable) => {
            tracing::error!("cannot create a session token: no OS randomness");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorBody {
                    error: "internal error".into(),
                }),
            )
                .into_response()
        }
        Err(LoginError::Locked(secs)) => {
            tracing::warn!(retry_after = secs, "login locked after repeated failures");
            let mut resp = (
                StatusCode::TOO_MANY_REQUESTS,
                Json(LockedBody {
                    error: "too many attempts".into(),
                    retry_after: secs,
                }),
            )
                .into_response();
            if let Ok(v) = HeaderValue::from_str(&secs.to_string()) {
                resp.headers_mut().insert(header::RETRY_AFTER, v);
            }
            resp
        }
    }
}

fn account_error(e: AccountError) -> Response {
    let (status, error) = match e {
        AccountError::Invalid => (StatusCode::FORBIDDEN, "wrong code or password"),
        AccountError::BadPassword => (
            StatusCode::BAD_REQUEST,
            "password must have 8 to 256 characters",
        ),
        AccountError::Conflict => (StatusCode::CONFLICT, "not available in this mode"),
        AccountError::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "internal error"),
        AccountError::Locked(secs) => {
            tracing::warn!(retry_after = secs, "locked after repeated failures");
            let mut resp = (
                StatusCode::TOO_MANY_REQUESTS,
                Json(LockedBody {
                    error: "too many attempts".into(),
                    retry_after: secs,
                }),
            )
                .into_response();
            if let Ok(v) = HeaderValue::from_str(&secs.to_string()) {
                resp.headers_mut().insert(header::RETRY_AFTER, v);
            }
            return resp;
        }
    };
    (
        status,
        Json(ErrorBody {
            error: error.into(),
        }),
    )
        .into_response()
}

/// Create the account on first run and log in.
///
/// Only possible while `setup_required` is true; the code comes from the server log.
#[utoipa::path(
    post,
    path = "/api/auth/setup",
    tag = "auth",
    request_body = SetupRequest,
    responses(
        (status = 200, description = "Account created and logged in; sets the `dlm_session` cookie", body = LoginResponse),
        (status = 400, description = "Password too short or too long", body = ErrorBody),
        (status = 403, description = "Wrong setup code", body = ErrorBody),
        (status = 409, description = "Setup already done, or authentication is configured externally", body = ErrorBody),
        (status = 429, description = "Too many failed attempts (5 per minute); see `Retry-After`", body = LockedBody),
    )
)]
pub(crate) async fn setup(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    Json(body): Json<SetupRequest>,
) -> Response {
    let worker = Arc::clone(&auth);
    let result = tokio::task::spawn_blocking(move || worker.setup(&body.code, &body.password))
        .await
        .unwrap_or(Err(AccountError::Internal));
    match result {
        Ok(token) => {
            tracing::info!("account created through the first-run setup");
            let mut resp = Json(LoginResponse {
                token: token.clone(),
            })
            .into_response();
            resp.headers_mut().insert(
                header::SET_COOKIE,
                session_cookie(&token, auth.ttl.as_secs(), &headers),
            );
            resp
        }
        Err(e) => {
            if e == AccountError::Invalid {
                tracing::warn!("wrong setup code");
            }
            account_error(e)
        }
    }
}

/// Change the password of an account created in the UI.
///
/// Ends every other session. Not available for a password from the environment or a file.
#[utoipa::path(
    post,
    path = "/api/auth/password",
    tag = "auth",
    security(("session" = []), ("bearer" = [])),
    request_body = ChangePasswordRequest,
    responses(
        (status = 204, description = "Password changed"),
        (status = 400, description = "New password too short or too long", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 403, description = "Wrong current password", body = ErrorBody),
        (status = 409, description = "The password is managed outside the UI", body = ErrorBody),
        (status = 429, description = "Too many failed attempts (5 per minute); see `Retry-After`", body = LockedBody),
    )
)]
pub(crate) async fn change_password(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    Json(body): Json<ChangePasswordRequest>,
) -> Response {
    let token = token_from(&headers);
    if !auth.is_valid(token.as_deref()) {
        return unauthorized();
    }
    let keep = token.unwrap_or_default();
    let worker = Arc::clone(&auth);
    let result = tokio::task::spawn_blocking(move || {
        worker.change_password(&keep, &body.current_password, &body.new_password)
    })
    .await
    .unwrap_or(Err(AccountError::Internal));
    match result {
        Ok(()) => {
            tracing::info!("password changed; other sessions ended");
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => account_error(e),
    }
}

/// End the current session (idempotent) and clear the cookie.
#[utoipa::path(
    post,
    path = "/api/auth/logout",
    tag = "auth",
    responses((status = 204, description = "Session ended"))
)]
pub(crate) async fn logout(State(auth): State<Arc<Auth>>, headers: HeaderMap) -> Response {
    if let Some(token) = token_from(&headers) {
        auth.logout(&token);
    }
    let mut resp = StatusCode::NO_CONTENT.into_response();
    resp.headers_mut()
        .insert(header::SET_COOKIE, session_cookie("", 0, &headers));
    resp
}

/// Whether login is required and whether the caller is already logged in.
#[utoipa::path(
    get,
    path = "/api/auth/status",
    tag = "auth",
    responses((status = 200, body = AuthStatus))
)]
pub(crate) async fn status(State(auth): State<Arc<Auth>>, headers: HeaderMap) -> Json<AuthStatus> {
    let authenticated = auth.is_valid(token_from(&headers).as_deref());
    Json(AuthStatus {
        auth_required: auth.required(),
        authenticated,
        setup_required: auth.setup_required(),
        can_change_password: auth.can_change_password(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correct_password_opens_a_session() {
        let a = Auth::with_password("hunter2hunter2");
        let t = a.login("hunter2hunter2").expect("login");
        assert_eq!(t.len(), 64);
        assert!(a.is_valid(Some(&t)));
        assert!(!a.is_valid(Some("nope")));
        assert!(!a.is_valid(None));
    }

    #[test]
    fn tokens_are_unique() {
        let a = Auth::with_password("pw");
        assert_ne!(a.login("pw").expect("1"), a.login("pw").expect("2"));
    }

    #[test]
    fn wrong_password_is_rejected() {
        let a = Auth::with_password("secret");
        assert_eq!(a.login("secre"), Err(LoginError::Invalid));
        assert_eq!(a.login("secret!"), Err(LoginError::Invalid));
        assert_eq!(a.login(""), Err(LoginError::Invalid));
    }

    #[test]
    fn repeated_failures_lock_even_the_right_password() {
        let a = Auth::with_password("secret");
        for _ in 0..MAX_FAILURES {
            assert_eq!(a.login("bad"), Err(LoginError::Invalid));
        }
        assert!(matches!(a.login("secret"), Err(LoginError::Locked(s)) if s >= 1));
    }

    #[test]
    fn success_resets_the_failure_counter() {
        let a = Auth::with_password("secret");
        for _ in 0..MAX_FAILURES - 1 {
            let _ = a.login("bad");
        }
        assert!(a.login("secret").is_ok());
        for _ in 0..MAX_FAILURES - 1 {
            assert_eq!(a.login("bad"), Err(LoginError::Invalid));
        }
    }

    #[test]
    fn logout_invalidates_the_token() {
        let a = Auth::with_password("pw");
        let t = a.login("pw").expect("login");
        a.logout(&t);
        assert!(!a.is_valid(Some(&t)));
        a.logout("unknown"); // no panic
    }

    #[test]
    fn sessions_expire() {
        let a = Auth::new(Mode::External("pw".into()), None, Duration::from_millis(30));
        let t = a.login("pw").expect("login");
        assert!(a.is_valid(Some(&t)));
        std::thread::sleep(Duration::from_millis(60));
        assert!(!a.is_valid(Some(&t)));
    }

    #[test]
    fn session_count_is_bounded() {
        let a = Auth::with_password("pw");
        for _ in 0..MAX_SESSIONS + 20 {
            a.login("pw").expect("login");
        }
        assert!(lock(&a.sessions).len() <= MAX_SESSIONS);
    }

    #[test]
    fn disabled_auth_lets_everything_through() {
        let a = Auth::disabled();
        assert!(!a.required());
        assert!(a.is_valid(None));
    }

    /// In-memory credential store.
    #[derive(Default)]
    struct MemStore(Mutex<Option<String>>);

    impl CredentialStore for MemStore {
        fn load_password_hash(&self) -> Result<Option<String>, String> {
            Ok(lock(&self.0).clone())
        }
        fn save_password_hash(&self, hash: &str) -> Result<(), String> {
            *lock(&self.0) = Some(hash.to_owned());
            Ok(())
        }
    }

    fn fresh() -> (Arc<MemStore>, Arc<Auth>) {
        let store = Arc::new(MemStore::default());
        let auth = Auth::from_store(store.clone()).expect("auth");
        (store, auth)
    }

    #[test]
    fn fresh_store_starts_in_setup_and_locks_everything() {
        let (_, a) = fresh();
        assert!(a.setup_required());
        assert!(a.required());
        assert!(!a.can_change_password());
        let code = a.setup_code().expect("code");
        assert_eq!(code.len(), CODE_LEN);
        assert!(code.bytes().all(|b| CODE_ALPHABET.contains(&b)));
        assert!(!a.is_valid(None));
        assert!(!a.is_valid(Some("anything")));
        // No account yet: no password, not even the code, works as a login.
        assert_eq!(a.login(&code), Err(LoginError::Invalid));
        assert_eq!(a.login(""), Err(LoginError::Invalid));
    }

    #[test]
    fn setup_creates_the_account_once() {
        let (store, a) = fresh();
        let code = a.setup_code().expect("code");
        let token = a.setup(&code, "correct-horse").expect("setup");
        assert!(a.is_valid(Some(&token)));
        assert!(!a.setup_required());
        assert!(a.can_change_password());
        assert_eq!(a.setup_code(), None);
        // The hash, not the password, was stored.
        let stored = store.load_password_hash().expect("load").expect("some");
        assert!(stored.starts_with("$argon2id$"));
        assert!(!stored.contains("correct-horse"));
        // A second setup (even with the right code) cannot replace the account.
        assert_eq!(a.setup(&code, "another-pass"), Err(AccountError::Conflict));
        assert!(a.login("correct-horse").is_ok());
        assert_eq!(a.login("another-pass"), Err(LoginError::Invalid));
    }

    #[test]
    fn setup_code_is_forgiving_about_how_it_is_typed() {
        let (_, a) = fresh();
        let code = a.setup_code().expect("code");
        let typed = format!(" {}-{} ", code[..4].to_lowercase(), &code[4..]);
        assert!(a.setup(&typed, "correct-horse").is_ok());
    }

    #[test]
    fn wrong_setup_code_and_weak_password_are_refused() {
        let (store, a) = fresh();
        let code = a.setup_code().expect("code");
        assert_eq!(
            a.setup("WRONG123", "correct-horse"),
            Err(AccountError::Invalid)
        );
        assert_eq!(a.setup("", "correct-horse"), Err(AccountError::Invalid));
        assert_eq!(a.setup(&code, "short"), Err(AccountError::BadPassword));
        assert_eq!(
            a.setup(&code, &"x".repeat(MAX_PASSWORD_LEN + 1)),
            Err(AccountError::BadPassword)
        );
        assert!(a.setup_required());
        assert_eq!(store.load_password_hash().expect("load"), None);
        // The right code still works afterwards.
        assert!(a.setup(&code, "correct-horse").is_ok());
    }

    #[test]
    fn setup_code_guesses_are_throttled() {
        let (_, a) = fresh();
        let code = a.setup_code().expect("code");
        for _ in 0..MAX_FAILURES {
            assert_eq!(
                a.setup("AAAAAAAA", "correct-horse"),
                Err(AccountError::Invalid)
            );
        }
        // Locked: even the correct code is refused until the window passes.
        assert!(matches!(
            a.setup(&code, "correct-horse"),
            Err(AccountError::Locked(s)) if s >= 1
        ));
        assert!(a.setup_required());
    }

    #[test]
    fn stored_account_survives_a_restart() {
        let (store, a) = fresh();
        let code = a.setup_code().expect("code");
        a.setup(&code, "correct-horse").expect("setup");
        let again = Auth::from_store(store).expect("auth");
        assert!(!again.setup_required());
        assert_eq!(again.setup_code(), None);
        assert!(again.login("correct-horse").is_ok());
        assert_eq!(again.login("wrong-horse!"), Err(LoginError::Invalid));
    }

    #[test]
    fn password_change_checks_the_old_one_and_ends_other_sessions() {
        let (_, a) = fresh();
        let code = a.setup_code().expect("code");
        let mine = a.setup(&code, "correct-horse").expect("setup");
        let other = a.login("correct-horse").expect("login");

        assert_eq!(
            a.change_password(&mine, "not-the-password", "new-password"),
            Err(AccountError::Invalid)
        );
        assert_eq!(
            a.change_password(&mine, "correct-horse", "short"),
            Err(AccountError::BadPassword)
        );
        assert!(a.is_valid(Some(&other)), "failed changes end nothing");

        a.change_password(&mine, "correct-horse", "new-password")
            .expect("change");
        assert!(a.is_valid(Some(&mine)));
        assert!(!a.is_valid(Some(&other)));
        assert_eq!(a.login("correct-horse"), Err(LoginError::Invalid));
        assert!(a.login("new-password").is_ok());
    }

    #[test]
    fn external_and_disabled_passwords_cannot_be_changed() {
        let ext = Auth::with_password("from-the-env");
        let t = ext.login("from-the-env").expect("login");
        assert!(!ext.can_change_password());
        assert!(!ext.setup_required());
        assert_eq!(
            ext.change_password(&t, "from-the-env", "new-password"),
            Err(AccountError::Conflict)
        );
        assert_eq!(
            ext.setup("ABCDEFGH", "new-password"),
            Err(AccountError::Conflict)
        );

        let off = Auth::disabled();
        assert_eq!(
            off.setup("ABCDEFGH", "new-password"),
            Err(AccountError::Conflict)
        );
        assert_eq!(
            off.change_password("", "x", "new-password"),
            Err(AccountError::Conflict)
        );
    }

    #[test]
    fn hashes_are_salted_and_garbage_hashes_never_verify() {
        let a = hash_password("correct-horse").expect("hash");
        let b = hash_password("correct-horse").expect("hash");
        assert_ne!(a, b);
        assert!(verify_hash(&a, "correct-horse"));
        assert!(!verify_hash(&a, "correct-horsE"));
        assert!(!verify_hash("not a hash", "correct-horse"));
        assert!(!verify_hash("", ""));
    }

    #[test]
    fn setup_codes_differ() {
        assert_ne!(new_setup_code(), new_setup_code());
    }

    #[test]
    fn constant_time_eq_handles_lengths() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(!constant_time_eq(b"", b"a"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn token_is_read_from_bearer_or_cookie() {
        let mut h = HeaderMap::new();
        h.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer abc"),
        );
        assert_eq!(token_from(&h).as_deref(), Some("abc"));
        let mut h = HeaderMap::new();
        h.insert(
            header::COOKIE,
            HeaderValue::from_static("theme=dark; dlm_session=xyz; other=1"),
        );
        assert_eq!(token_from(&h).as_deref(), Some("xyz"));
        assert_eq!(token_from(&HeaderMap::new()), None);
    }

    #[test]
    fn cookie_is_secure_only_behind_https() {
        let plain = session_cookie("t", 10, &HeaderMap::new());
        assert!(!plain.to_str().expect("s").contains("Secure"));
        assert!(plain.to_str().expect("s").contains("HttpOnly"));
        assert!(plain.to_str().expect("s").contains("SameSite=Strict"));
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-proto", HeaderValue::from_static("https"));
        assert!(session_cookie("t", 10, &h)
            .to_str()
            .expect("s")
            .contains("Secure"));
    }
}
