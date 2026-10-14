//! Single-user authentication (SPEC 6.7).
//!
//! One shared password. A successful login creates a random session token, delivered
//! as an `HttpOnly` cookie (needed for SSE, `EventSource` cannot set headers) and in the
//! JSON body for API clients that send `Authorization: Bearer <token>`.
//!
//! Sessions live in memory: a restart logs everybody out, which is fine for one user and
//! avoids persisting secrets. Failed logins are throttled globally (one user, and no
//! trusting of client IPs behind a reverse proxy).

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

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

/// Authentication state shared by all requests.
pub struct Auth {
    password: Option<String>,
    ttl: Duration,
    sessions: Mutex<HashMap<String, Instant>>,
    failures: Mutex<VecDeque<Instant>>,
}

impl Auth {
    /// No authentication: every request is allowed (explicit `--no-auth`).
    pub fn disabled() -> Arc<Self> {
        Arc::new(Self::new(None, SESSION_TTL))
    }

    /// Require `password` for everything except health and the auth endpoints.
    pub fn with_password(password: impl Into<String>) -> Arc<Self> {
        Arc::new(Self::new(Some(password.into()), SESSION_TTL))
    }

    fn new(password: Option<String>, ttl: Duration) -> Self {
        Self {
            password,
            ttl,
            sessions: Mutex::new(HashMap::new()),
            failures: Mutex::new(VecDeque::new()),
        }
    }

    /// Whether requests must be authenticated.
    pub fn required(&self) -> bool {
        self.password.is_some()
    }

    /// Check the password and open a session; returns its token.
    pub fn login(&self, attempt: &str) -> Result<String, LoginError> {
        let Some(password) = &self.password else {
            return Ok(String::new());
        };
        let now = Instant::now();
        {
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
                return Err(LoginError::Locked(wait.as_secs().max(1)));
            }
            if !constant_time_eq(password.as_bytes(), attempt.as_bytes()) {
                failures.push_back(now);
                return Err(LoginError::Invalid);
            }
            failures.clear();
        }

        let Some(token) = new_token() else {
            return Err(LoginError::Unavailable);
        };
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
        Ok(token)
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
    match auth.login(&body.password) {
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
        let a = Auth::new(Some("pw".into()), Duration::from_millis(30));
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
