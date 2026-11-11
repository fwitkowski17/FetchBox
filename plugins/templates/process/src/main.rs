//! The long-running program of the extension. FetchBox starts it with the extension and talks to it
//! in JSON lines: one object per line on standard input and output. Standard error is free text and
//! becomes the extension's log (Show log on its card).
//!
//! From FetchBox: `initialize`, `invoke` (a button), `event` (a hook), `settings_changed`,
//! `shutdown`. From here: answers to requests, and the notifications `log`, `status` and `notify`.
//! The protocol is described in plugins/README.md of the FetchBox repository.

use std::io::{BufRead, Write};

use serde_json::{json, Value};

/// What the program remembers between messages.
#[derive(Default)]
struct State {
    /// The settings of the extension, as FetchBox last sent them (a JSON object).
    settings: Value,
}

impl State {
    fn greeting(&self) -> &str {
        self.settings["greeting"].as_str().unwrap_or("pong")
    }
}

fn notification(method: &str, params: Value) -> Value {
    json!({ "method": method, "params": params })
}

fn log(level: &str, message: &str) -> Value {
    notification("log", json!({ "level": level, "message": message }))
}

/// The answer to request `id` (nothing for a notification, which has no `id`).
fn answer(id: Option<&Value>, result: Value) -> Option<Value> {
    id.map(|id| json!({ "id": id, "result": result }))
}

/// Handle one message from FetchBox. Returns the lines to send back and whether to go on.
fn handle(state: &mut State, message: &Value) -> (Vec<Value>, bool) {
    let id = message.get("id").filter(|id| !id.is_null());
    let params = &message["params"];
    let mut out = Vec::new();
    match message["method"].as_str().unwrap_or("") {
        "initialize" => {
            state.settings = params["settings"].clone();
            out.extend(answer(id, json!({})));
            out.push(notification("status", json!({ "text": "ready" })));
            out.push(log(
                "info",
                &format!(
                    "started, FetchBox {}",
                    params["fetchbox_version"].as_str().unwrap_or("?")
                ),
            ));
        }
        "settings_changed" => {
            state.settings = params["settings"].clone();
            out.push(log("info", "settings changed"));
        }
        "event" => {
            // The document of the hook: {"event": "download.completed", "at": ..., "download": {...}}
            let name = params["event"].as_str().unwrap_or("?");
            let file = params["download"]["filename"].as_str().unwrap_or("-");
            out.push(log("info", &format!("{name}: {file}")));
        }
        "invoke" => {
            // A button. params["action"] is its id, params["input"] the fields of its dialog.
            out.extend(answer(
                id,
                json!({ "ok": true, "message": state.greeting(), "level": "success" }),
            ));
        }
        "shutdown" => {
            out.extend(answer(id, json!({})));
            return (out, false);
        }
        other => {
            if let Some(id) = id {
                out.push(json!({
                    "id": id,
                    "error": { "code": -1, "message": format!("unknown method {other}") }
                }));
            }
        }
    }
    (out, true)
}

fn main() {
    let mut state = State::default();
    let mut stdout = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            eprintln!("not JSON: {}", line.chars().take(200).collect::<String>());
            continue;
        };
        let (lines, go_on) = handle(&mut state, &message);
        for line in lines {
            if writeln!(stdout, "{line}")
                .and_then(|()| stdout.flush())
                .is_err()
            {
                return; // FetchBox is gone
            }
        }
        if !go_on {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn send(state: &mut State, message: Value) -> (Vec<Value>, bool) {
        handle(state, &message)
    }

    #[test]
    fn initialize_is_answered_and_the_settings_are_kept() {
        let mut state = State::default();
        let (out, go_on) = send(
            &mut state,
            json!({"id": 1, "method": "initialize", "params": {"settings": {"greeting": "hi"}}}),
        );
        assert!(go_on);
        assert_eq!(out[0], json!({"id": 1, "result": {}}));
        assert_eq!(out[1]["params"]["text"], "ready");
        assert_eq!(state.greeting(), "hi");
    }

    #[test]
    fn a_button_answers_with_the_greeting_and_a_changed_setting_is_used_at_once() {
        let mut state = State::default();
        let (out, _) = send(
            &mut state,
            json!({"id": 2, "method": "invoke", "params": {"action": "ping"}}),
        );
        assert_eq!(out[0]["result"]["message"], "pong");

        send(
            &mut state,
            json!({"method": "settings_changed", "params": {"settings": {"greeting": "yo"}}}),
        );
        let (out, _) = send(
            &mut state,
            json!({"id": 3, "method": "invoke", "params": {}}),
        );
        assert_eq!(out[0]["result"]["message"], "yo");
    }

    #[test]
    fn an_event_is_logged_and_needs_no_answer() {
        let mut state = State::default();
        let (out, go_on) = send(
            &mut state,
            json!({"method": "event", "params": {"event": "download.completed", "download": {"filename": "a.iso"}}}),
        );
        assert!(go_on);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["params"]["message"], "download.completed: a.iso");
    }

    #[test]
    fn shutdown_ends_the_loop_and_an_unknown_request_is_an_error() {
        let mut state = State::default();
        let (out, go_on) = send(&mut state, json!({"id": 4, "method": "nope"}));
        assert!(go_on);
        assert_eq!(out[0]["error"]["message"], "unknown method nope");
        // A notification we do not know is ignored, not answered.
        assert!(send(&mut state, json!({"method": "nope"})).0.is_empty());

        let (out, go_on) = send(&mut state, json!({"id": 5, "method": "shutdown"}));
        assert!(!go_on);
        assert_eq!(out[0], json!({"id": 5, "result": {}}));
    }
}
