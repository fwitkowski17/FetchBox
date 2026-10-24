# __NAME__

A FetchBox extension made from the `process` template: FetchBox starts `bin/plugin.py` with the extension, keeps it
running, sends it the events of a hook and lets a button ask it something. This is the shape for a metrics exporter
or an MCP bridge.

## Try it

1. Put this folder inside `<data dir>/plugins/` and start FetchBox. For the dev backend that is `./.dev/data/plugins/`:
   `cargo run -p server -- --data-dir ./.dev/data --download-dir ./.dev/downloads`.
2. **Extensions → Rescan**, review the permissions, approve, switch it on. The card shows the line "ready".
3. Press **Ping**: the toast shows the *Greeting* setting. Let a download finish, then open **Show log**.

You can talk to the program by hand, the protocol is plain lines:

```sh
echo '{"id":1,"method":"initialize","params":{"settings":{"greeting":"hi"}}}' | ./bin/plugin.py
```

Run FetchBox with `--log-activity` to see the extension start and stop. Editing `plugin.json` puts the extension
back to *needs review*; editing `bin/plugin.py` does not (restart it from the card).

## Where to go from here

- More events in the hook: `download.*`, `torrent.*`, `*` (progress events must be named, and `every_s` thins them).
- A dialog for the button: add `ui.dialogs` and `open_dialog` (see `plugins/official/webhook-notify`).
- A program that is not a script: set `process.command` to a `${dep:...}` and list the program under `dependencies`
  with its SHA-256. Python is not in the official Docker image.

The full guide is `plugins/README.md` in the FetchBox repository.
