#!/usr/bin/env python3
"""The long-running program of the extension: FetchBox starts it and talks to it in JSON lines,
one object per line, on standard input and output. Standard error is free text and becomes the
extension's log (Show log on its card).

From FetchBox: `initialize`, `invoke` (a button), `event` (a hook), `settings_changed`, `shutdown`.
From here: answers to requests, and the notifications `log`, `status` and `notify`.

Python 3 only, no packages. The official Docker image has no interpreter: there, ship a static
program instead (see plugins/README.md).
"""
import json
import sys

settings = {}


def send(message):
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()


def log(text, level="info"):
    send({"method": "log", "params": {"level": level, "message": text}})


def handle(message):
    global settings
    method = message.get("method")
    params = message.get("params") or {}
    request_id = message.get("id")

    if method == "initialize":
        settings = params.get("settings") or {}
        send({"id": request_id, "result": {}})
        send({"method": "status", "params": {"text": "ready"}})
        log("started, FetchBox " + str(params.get("fetchbox_version")))
    elif method == "settings_changed":
        settings = params.get("settings") or {}
        log("settings changed: " + ", ".join(sorted(settings)))
    elif method == "event":
        # The document of the hook: {"event": "download.completed", "at": ..., "download": {...}}
        name = params.get("event")
        download = params.get("download") or {}
        log("{}: {}".format(name, download.get("filename")))
    elif method == "invoke":
        # A button. `params["action"]` is its id, `params["input"]` the fields of its dialog.
        send({
            "id": request_id,
            "result": {"ok": True, "message": settings.get("greeting") or "pong", "level": "success"},
        })
    elif method == "shutdown":
        send({"id": request_id, "result": {}})
        return False
    elif request_id is not None:
        send({"id": request_id, "error": {"code": -1, "message": "unknown method " + str(method)}})
    return True


def main():
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            message = json.loads(line)
        except ValueError:
            log("not JSON: " + line[:200], "warning")
            continue
        if not handle(message):
            break


if __name__ == "__main__":
    main()
