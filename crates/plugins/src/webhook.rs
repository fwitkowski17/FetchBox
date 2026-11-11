//! Webhooks: requests FetchBox sends itself on a plugin's behalf, so a plugin that only wants to
//! tell a service about an event needs no program at all (SPEC 6.15).

use std::time::Duration;

use fetchbox_core::plugin::{Webhook, WebhookBody};
use serde_json::Value;

use crate::expand::{self, Context};
use crate::fetch::Fetcher;
use crate::{Error, Result};

const TIMEOUT: Duration = Duration::from_secs(10);

fn value_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Fill in `{{path}}` in a template from a JSON document: `{{download.filename}}`. A string is
/// inserted as it is, anything else as JSON, a missing path as nothing. An unclosed `{{` stays.
pub fn render_template(template: &str, doc: &Value) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let path = after[..end].trim();
        let value = path
            .split('.')
            .try_fold(doc, |v, key| v.get(key))
            .map(value_text)
            .unwrap_or_default();
        out.push_str(&value);
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

/// Send a webhook. `doc` is the event (or the action) it reports; the address, the headers and the
/// template may refer to the plugin's settings (`${setting:key}`). Returns the HTTP status.
pub async fn send(
    fetcher: &Fetcher,
    hook: &Webhook,
    ctx: &Context<'_>,
    doc: &Value,
) -> Result<u16> {
    let url = expand::expand_string(&hook.url, ctx)?;
    if url.trim().is_empty() {
        return Err(Error::Run(
            "the webhook address is empty: set it in the plugin's settings".into(),
        ));
    }
    let mut headers = Vec::new();
    for (k, v) in &hook.headers {
        // A header whose setting is empty (an optional token) is left out, not sent empty.
        let value = expand::expand_string(v, ctx)?;
        if !value.trim().is_empty() {
            headers.push((k.clone(), value));
        }
    }
    let (body, content_type) = match hook.body {
        WebhookBody::Event => (doc.to_string(), "application/json"),
        WebhookBody::Template => {
            let template = hook.template.as_deref().unwrap_or_default();
            // Settings first, then the event's fields.
            let filled = expand::expand_string(
                &template.replace("{{", "\u{1}{").replace("}}", "}\u{1}"),
                ctx,
            )
            .map(|s| s.replace("\u{1}{", "{{").replace("}\u{1}", "}}"))?;
            (render_template(&filled, doc), "text/plain; charset=utf-8")
        }
    };
    let method = hook.method.as_deref().unwrap_or("POST");
    fetcher
        .send(
            method,
            &url,
            &headers,
            body.into_bytes(),
            content_type,
            TIMEOUT,
        )
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode};
    use axum::routing::post;
    use axum::Router;
    use fetchbox_core::plugin::SettingValue;
    use fetchbox_http_engine::UrlPolicy;
    use serde_json::json;

    #[test]
    fn templates_are_filled_from_the_document() {
        let doc = json!({"download": {"filename": "a.iso", "size": 5, "ok": true}, "n": null});
        assert_eq!(
            render_template("{{download.filename}} ({{download.size}})", &doc),
            "a.iso (5)"
        );
        assert_eq!(render_template("{{ download.ok }}", &doc), "true");
        assert_eq!(
            render_template("[{{missing}}][{{n}}][{{download.x.y}}]", &doc),
            "[][][]"
        );
        assert_eq!(render_template("no placeholders", &doc), "no placeholders");
        assert_eq!(
            render_template("open {{ never closed", &doc),
            "open {{ never closed"
        );
        assert_eq!(
            render_template("{{download}}", &doc),
            r#"{"filename":"a.iso","ok":true,"size":5}"#
        );
        assert_eq!(
            render_template("zażółć {{download.filename}}", &doc),
            "zażółć a.iso"
        );
    }

    type Seen = Arc<Mutex<Vec<(String, String, Option<String>)>>>;

    async fn receiver(status: StatusCode) -> (String, Seen) {
        let seen: Seen = Arc::default();
        let app = Router::new()
            .route(
                "/hook",
                post(
                    |State((seen, status)): State<(Seen, StatusCode)>,
                     h: HeaderMap,
                     body: String| async move {
                        let ct = h
                            .get("content-type")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("")
                            .to_owned();
                        let auth = h
                            .get("x-token")
                            .and_then(|v| v.to_str().ok())
                            .map(str::to_owned);
                        seen.lock().expect("lock").push((ct, body, auth));
                        status
                    },
                ),
            )
            .with_state((seen.clone(), status));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let base = format!("http://{}", listener.local_addr().expect("addr"));
        tokio::spawn(async move { axum::serve(listener, app).await });
        (base, seen)
    }

    fn ctx<'a>(
        settings: &'a BTreeMap<String, SettingValue>,
        deps: &'a BTreeMap<String, expand::DepPaths>,
    ) -> Context<'a> {
        Context {
            plugin_dir: Path::new("/p"),
            data_dir: Path::new("/d"),
            tmp: Path::new("/d/tmp"),
            settings,
            deps,
            url: None,
            choice_args: &[],
            out_dir: None,
        }
    }

    fn hook(url: &str, body: WebhookBody, template: Option<&str>) -> Webhook {
        Webhook {
            url: url.into(),
            method: None,
            headers: [("X-Token".to_owned(), "${setting:token}".to_owned())].into(),
            body,
            template: template.map(str::to_owned),
        }
    }

    #[tokio::test]
    async fn an_event_is_posted_as_json_with_headers_from_the_settings() {
        let (base, seen) = receiver(StatusCode::OK).await;
        let settings: BTreeMap<_, _> = [
            ("url".to_owned(), SettingValue::Text(format!("{base}/hook"))),
            ("token".to_owned(), SettingValue::Text("s3cret".into())),
        ]
        .into();
        let deps = BTreeMap::new();
        let f = Fetcher::new(UrlPolicy::default()).expect("fetcher");
        let doc = json!({"event": "download.completed", "download": {"filename": "a.iso"}});
        let status = send(
            &f,
            &hook("${setting:url}", WebhookBody::Event, None),
            &ctx(&settings, &deps),
            &doc,
        )
        .await
        .expect("sent");
        assert_eq!(status, 200);
        let seen = seen.lock().expect("lock");
        let (ct, body, token) = &seen[0];
        assert_eq!(ct, "application/json");
        assert_eq!(serde_json::from_str::<Value>(body).expect("json"), doc);
        assert_eq!(token.as_deref(), Some("s3cret"));
    }

    #[tokio::test]
    async fn a_template_body_can_use_settings_and_event_fields() {
        let (base, seen) = receiver(StatusCode::OK).await;
        let settings: BTreeMap<_, _> = [
            ("token".to_owned(), SettingValue::Text("t".into())),
            ("who".to_owned(), SettingValue::Text("Filip".into())),
        ]
        .into();
        let deps = BTreeMap::new();
        let f = Fetcher::new(UrlPolicy::default()).expect("fetcher");
        let doc = json!({"download": {"filename": "a.iso"}});
        let h = hook(
            &format!("{base}/hook"),
            WebhookBody::Template,
            Some("Hi ${setting:who}: {{download.filename}} is done"),
        );
        send(&f, &h, &ctx(&settings, &deps), &doc)
            .await
            .expect("sent");
        let seen = seen.lock().expect("lock");
        assert_eq!(seen[0].0, "text/plain; charset=utf-8");
        assert_eq!(seen[0].1, "Hi Filip: a.iso is done");
    }

    #[tokio::test]
    async fn problems_are_reported_not_hidden() {
        let (base, _) = receiver(StatusCode::INTERNAL_SERVER_ERROR).await;
        let settings: BTreeMap<_, _> =
            [("token".to_owned(), SettingValue::Text("t".into()))].into();
        let deps = BTreeMap::new();
        let f = Fetcher::new(UrlPolicy::default()).expect("fetcher");
        let c = ctx(&settings, &deps);
        let doc = json!({});
        let e = send(
            &f,
            &hook(&format!("{base}/hook"), WebhookBody::Event, None),
            &c,
            &doc,
        )
        .await
        .expect_err("500");
        assert!(e.to_string().contains("500"), "{e}");
        let e = send(
            &f,
            &hook("${setting:url}", WebhookBody::Event, None),
            &c,
            &doc,
        )
        .await
        .expect_err("empty");
        assert!(e.to_string().contains("address is empty"), "{e}");
        let e = send(&f, &hook("ftp://x/", WebhookBody::Event, None), &c, &doc)
            .await
            .expect_err("scheme");
        assert!(e.to_string().contains("not an https"), "{e}");
        // Private addresses are refused when the server is told to block them.
        let guarded = Fetcher::new(UrlPolicy {
            block_private: true,
        })
        .expect("fetcher");
        let e = send(
            &guarded,
            &hook(&format!("{base}/hook"), WebhookBody::Event, None),
            &c,
            &doc,
        )
        .await
        .expect_err("private");
        assert!(e.to_string().contains("blocked"), "{e}");
    }
}
