# __NAME__

A FetchBox extension made from the `hook` template: it sends a message to a web address when a download finishes or
fails, and has a "Send a test message" button. No program of its own, everything is in `plugin.json`.

## Try it

1. Start FetchBox with this folder inside `<data dir>/plugins/` (`scripts/new-plugin.sh --dev` puts it in
   `./.dev/data/plugins/` for `cargo run -p server -- --data-dir ./.dev/data --download-dir ./.dev/downloads`).
2. **Extensions → Rescan**, review the permissions, approve, switch it on.
3. **Settings → Extensions**: enter an address (a free topic on <https://ntfy.sh> is the quickest) and save.
4. Press **Send a test message** on the card, then let a download finish.

Editing `plugin.json` puts the extension back to *needs review*: press **Review** again.

## Where to go from here

- Other events: `download.added`, `download.started`, `torrent.completed`, ... or `*` (list in `plugins/README.md`).
- The message: `{{download.filename}}`, `{{download.size}}`, any field of the event document; or
  `"body": "event"` to send the whole document as JSON.
- Package it with `cd __ID__ && zip -r ../__ID__-0.1.0.zip .` and use **Install from file**.

The full guide is `plugins/README.md` in the FetchBox repository.
