//! The conversation with a plugin's long-lived program: one JSON object per line over its standard
//! input and output (SPEC 6.15).
//!
//! From FetchBox: requests `{"id":1,"method":"resolve","params":{...}}` (answered by exactly one
//! response) and notifications `{"method":"event","params":{...}}` (no answer).
//! From the plugin: responses `{"id":1,"result":...}` or `{"id":1,"error":{"code":..,"message":..}}`,
//! and notifications (`log`, `status`, `notify`). A request from the plugin (`id` and `method`) is
//! answered with "not implemented" for now; the names are reserved for calls into FetchBox.

use serde_json::{json, Value};

/// Longest line either side may send.
pub const MAX_LINE: usize = 1024 * 1024;

/// An error a plugin answered a request with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

/// A line from the plugin.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    Response {
        id: u64,
        result: Result<Value, RpcError>,
    },
    Notification {
        method: String,
        params: Value,
    },
    /// A call into FetchBox (reserved).
    Request {
        id: Value,
        method: String,
        params: Value,
    },
}

/// Read one line sent by the plugin.
pub fn parse_line(line: &str) -> Result<Incoming, String> {
    let v: Value = serde_json::from_str(line).map_err(|e| format!("not JSON: {e}"))?;
    let Value::Object(mut o) = v else {
        return Err("not a JSON object".into());
    };
    let method = o.get("method").and_then(Value::as_str).map(str::to_owned);
    let params = o.remove("params").unwrap_or(Value::Null);
    match (method, o.get("id").cloned()) {
        (Some(method), None) => Ok(Incoming::Notification { method, params }),
        (Some(method), Some(id)) => Ok(Incoming::Request { id, method, params }),
        (None, Some(id)) => {
            let id = id
                .as_u64()
                .ok_or("a response with an id that is not a number")?;
            if let Some(err) = o.remove("error") {
                let code = err.get("code").and_then(Value::as_i64).unwrap_or(-32000);
                let message = err
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("the plugin reported an error")
                    .to_owned();
                Ok(Incoming::Response {
                    id,
                    result: Err(RpcError { code, message }),
                })
            } else if let Some(result) = o.remove("result") {
                Ok(Incoming::Response {
                    id,
                    result: Ok(result),
                })
            } else {
                Err("a response with neither `result` nor `error`".into())
            }
        }
        (None, None) => Err("neither a request, a response nor a notification".into()),
    }
}

fn line(v: &Value) -> String {
    let mut s = v.to_string();
    s.push('\n');
    s
}

/// A request line for the plugin.
pub fn request_line(id: u64, method: &str, params: &Value) -> String {
    line(&json!({"id": id, "method": method, "params": params}))
}

/// A notification line for the plugin.
pub fn notification_line(method: &str, params: &Value) -> String {
    line(&json!({"method": method, "params": params}))
}

/// The answer to a request the plugin made.
pub fn error_response_line(id: &Value, code: i64, message: &str) -> String {
    line(&json!({"id": id, "error": {"code": code, "message": message}}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responses_requests_and_notifications_are_told_apart() {
        assert_eq!(
            parse_line(r#"{"id":3,"result":{"ok":true}}"#).expect("ok"),
            Incoming::Response {
                id: 3,
                result: Ok(json!({"ok": true}))
            }
        );
        assert_eq!(
            parse_line(r#"{"id":3,"result":null}"#).expect("ok"),
            Incoming::Response {
                id: 3,
                result: Ok(Value::Null)
            }
        );
        assert_eq!(
            parse_line(r#"{"id":4,"error":{"code":-5,"message":"nope"}}"#).expect("ok"),
            Incoming::Response {
                id: 4,
                result: Err(RpcError {
                    code: -5,
                    message: "nope".into()
                })
            }
        );
        assert_eq!(
            parse_line(r#"{"id":4,"error":{}}"#).expect("ok"),
            Incoming::Response {
                id: 4,
                result: Err(RpcError {
                    code: -32000,
                    message: "the plugin reported an error".into()
                })
            }
        );
        assert_eq!(
            parse_line(r#"{"method":"log","params":{"level":"info","message":"hi"}}"#).expect("ok"),
            Incoming::Notification {
                method: "log".into(),
                params: json!({"level": "info", "message": "hi"})
            }
        );
        assert_eq!(
            parse_line(r#"{"method":"status"}"#).expect("ok"),
            Incoming::Notification {
                method: "status".into(),
                params: Value::Null
            }
        );
        assert_eq!(
            parse_line(r#"{"id":"a","method":"downloads.list","params":{}}"#).expect("ok"),
            Incoming::Request {
                id: json!("a"),
                method: "downloads.list".into(),
                params: json!({})
            }
        );
    }

    #[test]
    fn nonsense_is_refused_with_a_reason() {
        for bad in [
            "",
            "hello",
            "[1]",
            "42",
            "{}",
            r#"{"id":1}"#,
            r#"{"id":"x","result":1}"#,
            r#"{"id":-1,"result":1}"#,
        ] {
            assert!(parse_line(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn lines_end_with_a_newline_and_round_trip() {
        let l = request_line(7, "resolve", &json!({"url": "https://x/\nnew"}));
        assert!(l.ends_with('\n'));
        assert_eq!(
            l.matches('\n').count(),
            1,
            "the newline inside the value is escaped"
        );
        let back: Value = serde_json::from_str(&l).expect("json");
        assert_eq!(back["id"], 7);
        assert_eq!(back["params"]["url"], "https://x/\nnew");
        let n = notification_line("event", &json!({"name": "download.completed"}));
        assert!(n.ends_with('\n') && !n.contains("\"id\""));
        let e = error_response_line(&json!(9), -32601, "not implemented");
        assert!(e.contains("-32601") && e.contains("not implemented"));
    }
}
