# __NAME__

A FetchBox extension: it sends a message to a web address when a download finishes or fails, and has a "Send a test
message" button. It has no program of its own, everything is in `plugin.json`; the project is meant to live in a
repository of its own:

```
plugin.json                      the manifest FetchBox reads
.github/workflows/ci.yml         checks that plugin.json is JSON
.github/workflows/release.yml    on a tag: publishes the plugin ZIP
```

## Develop

```sh
fetchbox --data-dir <data dir of your FetchBox> plugin link .
# or, from the root of a FetchBox checkout (dev backend): cargo plugin-link path/to/__ID__
```

`plugin link` copies `plugin.json` into `<data dir>/plugins/__ID__/`. Then **Extensions → Rescan**, review the permissions,
approve and switch it on. In **Settings → Extensions** enter an address (a free topic on <https://ntfy.sh> is the quickest) and
save. Press **Send a test message** on the card, then let a download finish.

After a change to `plugin.json`: run `plugin link` again and approve the extension again.

## Where to go from here

- Other events: `download.added`, `download.started`, `torrent.completed`, ... or `*` (the list is in
  `plugins/README.md` of the FetchBox repository).
- The message: `{{download.filename}}`, `{{download.size}}`, any field of the event document; or `"body": "event"` to
  send the whole document as JSON.
- Your own program for more than a request: start from `fetchbox plugin new <id> -t process`.

## Publish

1. Create the repository and set it in `plugin.json`: `homepage` should be its address (`https://github.com/__REPO__` now).
2. Push, then tag a release. The tag is the version in `plugin.json`: `git tag v0.1.0 && git push origin v0.1.0`.
3. The workflow publishes the release with `__ID__-0.1.0.zip`. Users install it with **Extensions → Install from file**. To
   list it in the store, add an entry to `plugins/index.json` of FetchBox with the address of the ZIP and its SHA-256 (the
   job summary of the release prints it); see `plugins/README.md` in the FetchBox repository.

The full guide to what a manifest can do is `plugins/README.md` in the FetchBox repository.
