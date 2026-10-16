//! OpenAPI document (`utoipa`), served at `/api/openapi.json` and printed by
//! `fetchbox --print-openapi`. The frontend generates its types from it.

use axum::Json;
use utoipa::openapi::security::{ApiKey, ApiKeyValue, HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};

use crate::auth::{
    self, AuthStatus, ChangePasswordRequest, LockedBody, LoginRequest, LoginResponse, SetupRequest,
};
use crate::error::{ConflictBody, ErrorBody, SpaceBody};
use crate::routes::{self, Health, UpdateDownload};
use crate::sse;

use fetchbox_core::{
    AddRequest, Category, CategoryStat, ChecksumAlgo, ChecksumInput, ChecksumState, Conflict,
    ConflictKind, DayStat, DiskSpace, Download, Event, Hold, LargestFile, ListingEntry, OnConflict,
    OnHoursEnd, ProbeRequest, ReleaseNote, Settings, SourceKind, Stats, StatsRange, Status,
    UrlInfo, UrlKind, VersionInfo,
};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "FetchBox API",
        description = "Self-hosted download manager. Commands over REST, live updates over SSE.\n\n\
Everything except `/api/health`, `/api/openapi.json` and `/api/auth/*` needs a session: \
log in with `POST /api/auth/login` (sets the `fetchbox_session` cookie) or send the returned \
token as `Authorization: Bearer`.",
    ),
    paths(
        routes::health,
        routes::list,
        routes::add,
        routes::probe,
        routes::update,
        routes::remove,
        routes::pause,
        routes::resume,
        routes::cancel,
        routes::keep,
        routes::get_settings,
        routes::put_settings,
        routes::disk,
        routes::stats,
        routes::version,
        routes::check_version,
        sse::events,
        auth::login,
        auth::logout,
        auth::status,
        auth::setup,
        auth::change_password,
    ),
    components(schemas(
        Download, Status, Category, SourceKind, Settings, AddRequest, OnConflict, Conflict,
        ConflictKind, Event, UpdateDownload, Health, ErrorBody, ConflictBody, SpaceBody, DiskSpace, LoginRequest,
        LoginResponse, LockedBody, AuthStatus, SetupRequest, ChangePasswordRequest, Stats, StatsRange,
        LargestFile, CategoryStat, DayStat, ProbeRequest, UrlInfo, UrlKind, ListingEntry, VersionInfo, ReleaseNote, Hold, OnHoursEnd, ChecksumAlgo, ChecksumInput, ChecksumState,
    )),
    modifiers(&Runtime),
    tags(
        (name = "downloads", description = "Download list and per-download commands"),
        (name = "settings", description = "Global settings, applied live"),
        (name = "events", description = "Server-sent events"),
        (name = "auth", description = "Single-user login, first-run setup, password change"),
        (name = "system", description = "Health"),
    )
)]
pub struct ApiDoc;

/// Things `#[openapi]` cannot express: security schemes and the crate version.
struct Runtime;

impl Modify for Runtime {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        openapi.info.version = env!("CARGO_PKG_VERSION").to_owned();
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "session",
            SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new("fetchbox_session"))),
        );
        components.add_security_scheme(
            "bearer",
            SecurityScheme::Http(HttpBuilder::new().scheme(HttpAuthScheme::Bearer).build()),
        );
    }
}

/// The document as pretty JSON with a trailing newline (stable for committing).
pub fn spec_json() -> Result<String, serde_json::Error> {
    let mut json = ApiDoc::openapi().to_pretty_json()?;
    json.push('\n');
    Ok(json)
}

/// `GET /api/openapi.json`.
pub(crate) async fn serve() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> serde_json::Value {
        serde_json::from_str(&spec_json().expect("json")).expect("parse")
    }

    #[test]
    fn documents_every_route() {
        let d = doc();
        let paths = d["paths"].as_object().expect("paths");
        for (path, method) in [
            ("/api/health", "get"),
            ("/api/downloads", "get"),
            ("/api/downloads", "post"),
            ("/api/probe", "post"),
            ("/api/downloads/{id}", "patch"),
            ("/api/downloads/{id}", "delete"),
            ("/api/downloads/{id}/pause", "post"),
            ("/api/downloads/{id}/resume", "post"),
            ("/api/downloads/{id}/cancel", "post"),
            ("/api/downloads/{id}/keep", "post"),
            ("/api/settings", "get"),
            ("/api/settings", "put"),
            ("/api/disk", "get"),
            ("/api/version", "get"),
            ("/api/version/check", "post"),
            ("/api/events", "get"),
            ("/api/auth/login", "post"),
            ("/api/auth/logout", "post"),
            ("/api/auth/status", "get"),
            ("/api/auth/setup", "post"),
            ("/api/auth/password", "post"),
        ] {
            assert!(
                paths[path][method].is_object(),
                "{method} {path} is undocumented"
            );
        }
    }

    #[test]
    fn declares_security_schemes_and_protects_downloads() {
        let d = doc();
        assert_eq!(
            d["components"]["securitySchemes"]["session"]["in"],
            "cookie"
        );
        assert_eq!(
            d["components"]["securitySchemes"]["bearer"]["scheme"],
            "bearer"
        );
        assert!(d["paths"]["/api/downloads"]["get"]["security"].is_array());
        assert!(d["paths"]["/api/health"]["get"]["security"].is_null());
    }

    #[test]
    fn conflict_contract_is_visible_to_clients() {
        let d = doc();
        let post = &d["paths"]["/api/downloads"]["post"]["responses"];
        assert!(post["409"]["content"]["application/json"]["schema"]["$ref"]
            .as_str()
            .is_some_and(|r| r.ends_with("/ConflictBody")));
        let kinds = &d["components"]["schemas"]["ConflictKind"]["enum"];
        assert_eq!(
            kinds,
            &serde_json::json!(["duplicate_url", "file_exists", "name_reserved"])
        );
        let modes = &d["components"]["schemas"]["OnConflict"]["enum"];
        assert_eq!(modes, &serde_json::json!(["reject", "rename", "overwrite"]));
    }

    #[test]
    fn sse_payload_is_documented() {
        let d = doc();
        let ok = &d["paths"]["/api/events"]["get"]["responses"]["200"]["content"];
        assert!(ok["text/event-stream"].is_object());
        assert!(d["components"]["schemas"]["Event"].is_object());
    }
}
