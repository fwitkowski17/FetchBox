# Writing a FetchBox extension

An extension (the code calls it a *plugin*) adds something to FetchBox without changing FetchBox: a source of
downloads (YouTube through yt-dlp, a page that links to files), a reaction to events (a message when a download
finishes, metrics for VictoriaMetrics), a program that runs next to FetchBox (an MCP bridge), or just a few buttons
and settings of its own.

An extension is **a folder with a `plugin.json`** and, if it needs them, scripts or programs. There is no SDK to
learn and nothing to compile: the manifest says what the extension needs and what it adds, and FetchBox does the
rest (downloads the programs it depends on, draws its settings and dialogs, runs its commands, sends its webhooks).

- [What an extension can do](#what-an-extension-can-do)
- [Start from a template](#start-from-a-template)
- [Quick start: a notification in 15 lines](#quick-start)
- [Folder, ZIP and where things live](#folder-zip-and-where-things-live)
- [The manifest](#the-manifest) (`plugin.json`)
  - [Identity and platforms](#identity-and-platforms)
  - [Dependencies: programs FetchBox downloads for you](#dependencies)
  - [Settings](#settings)
  - [Templates for commands](#templates-for-commands)
  - [Resolvers: new sources of downloads](#resolvers)
  - [Hooks: reacting to events](#hooks)
  - [A program that keeps running](#a-program-that-keeps-running)
  - [Interface: actions, dialogs, a tab in Settings](#interface)
  - [Translations](#translations)
- [Permissions and the trust model](#permissions-and-the-trust-model)
- [Testing on your machine](#testing-on-your-machine)
- [Packaging and the store](#packaging-and-the-store)
- [Ideas: VictoriaMetrics and MCP](#ideas-victoriametrics-and-mcp)
- [Limits](#limits)

The two extensions in [`official/`](official) are complete examples: `webhook-notify` (settings, a dialog and a
hook, no program at all) and `yt-dlp` (dependencies, a resolver with presets, and a download done by the
extension's own program).

## What an extension can do

| You want to... | Use | Needs a program of your own? |
| --- | --- | --- |
| Download a program that your extension runs | [`dependencies`](#dependencies) | no (FetchBox fetches and checks it) |
| Offer settings | [`settings`](#settings) | no |
| Understand addresses of some sites (a video page, a gallery) | [`resolvers`](#resolvers) | a command that prints JSON about the address |
| Make the file with your own program instead of FetchBox's engine | a resolver with `run` | yes (usually a dependency such as yt-dlp) |
| Tell a service when a download or torrent changes | [`hooks`](#hooks) with a `webhook` | no |
| Run a command on an event | a hook with `run` | yes |
| Keep a program running and send it every event | [`process`](#a-program-that-keeps-running) | yes |
| Add buttons, dialogs or a page in Settings | [`ui`](#interface) | no |

Everything is described in JSON. The interface is *declarative*: you list fields, buttons and dialogs, FetchBox
draws them (so an extension can never run script in the page that holds the user's session).

## Start from a template

`scripts/new-plugin.sh` (bash; run it from a checkout of this repository) makes the folder of a new extension with a
`plugin.json` that already validates, a `README.md` that says how to try it, and the scripts it needs:

```sh
scripts/new-plugin.sh my-notifier --dev                     # a hook with a settings field and a test button
scripts/new-plugin.sh page-links -t resolver -n "Page links"  # Add download reads a page and lists its files
scripts/new-plugin.sh my-bridge  -t process --dev           # a program that keeps running (JSON lines)
scripts/new-plugin.sh --list                                # the templates
```

| Template | Makes | Needs |
| --- | --- | --- |
| `hook` (default) | a webhook on `download.completed` and `download.failed`, two settings, a dialog with a test button, a Polish translation | nothing |
| `resolver` | a resolver whose `bin/probe.py` lists the links of a page; the sites to read are a setting | Python 3 |
| `process` | `bin/plugin.py`, a long-running program that answers `initialize`, `invoke`, `event` and `settings_changed` | Python 3 |

Options: `-t/--template`, `-n/--name` (default: the id in words), `-a/--author` (default: `git config user.name`),
`-o/--out DIR` (where to create `<id>/`, default the current directory) and `--dev`, which creates it in
`./.dev/data/plugins`, where the dev backend of [Testing on your machine](#testing-on-your-machine) looks. It refuses an
invalid id and a folder that exists. Then **Extensions → Rescan**. The templates live in [`templates/`](templates)
(`__ID__`, `__NAME__` and `__AUTHOR__` are replaced); a test scaffolds each of them and validates the result.

The Python scripts are starting points for a host install. The official Docker image has no interpreter (see
[Folder, ZIP and where things live](#folder-zip-and-where-things-live)).

## Quick start

An extension that sends a request to a web address when a download finishes. Make a folder `hello-hook` with this
`plugin.json`:

```json
{
  "api_version": 1,
  "id": "hello-hook",
  "name": "Hello hook",
  "version": "1.0.0",
  "description": "Posts a message to ntfy when a download finishes.",
  "author": "You",
  "permissions": {
    "uses": ["network", "events"],
    "reasons": {
      "network": "Sends the message to the address you enter.",
      "events": "Needs to know when a download finishes."
    }
  },
  "settings": [
    { "key": "topic_url", "type": "text", "label": "ntfy address", "placeholder": "https://ntfy.sh/my-topic", "required": true }
  ],
  "hooks": [
    {
      "on": ["download.completed"],
      "action": { "webhook": { "url": "${setting:topic_url}", "body": "template", "template": "Finished: {{download.filename}}" } }
    }
  ]
}
```

Put the folder into `<data dir>/plugins/` (the data dir is the one given with `--data-dir`; `/var/lib/fetchbox`
for the packages, `/data` in Docker), open **Extensions**, press **Rescan**, review what it asks for (the groups you declared in `permissions.uses`), approve it,
switch it on, and set its address in **Settings → Extensions**. Or zip the *contents* of the folder and use
**Install from file**.

## Folder, ZIP and where things live

```
hello-hook/
  plugin.json        required; the only file FetchBox reads on its own
  bin/               your scripts or programs (optional)
  ...                anything else your commands need
```

- A ZIP has `plugin.json` at its top, or inside its one top-level folder (what GitHub's "Download ZIP" makes).
  A ZIP is at most 32 MiB; archives with absolute paths, `..`, links, too many files or an absurd compression ratio
  are refused. Zips and tar archives are unpacked by FetchBox, never by a shell.
- The installed files are `<data>/plugins/<id>/`. They are the extension's *code*: **treat them as read-only**, they
  are replaced on update. The folder name must equal the `id`.
- The extension's own data (downloaded programs, temporary files, a `home` folder, state) is
  `<data>/plugin-data/<id>/`, which survives updates and is removed on uninstall (unless the user keeps it).
  Programs get it as `HOME`, `TMPDIR` and `$FETCHBOX_PLUGIN_DATA`. Write only there.
- A program of yours runs with a minimal environment: `PATH` (the folders of its dependencies first), `HOME`,
  `TMPDIR`, `LANG`, `FETCHBOX_PLUGIN_ID`, `FETCHBOX_PLUGIN_DIR`, `FETCHBOX_PLUGIN_DATA` and `FETCHBOX_API_VERSION`.
  It does **not** get FetchBox's own environment (the password included).
- **The official Docker image has no shell and no interpreter** (it is distroless). A script that starts with
  `#!/bin/sh` or `python3` cannot run there. Ship a static binary, or make it a [dependency](#dependencies). On a
  host install (packages, `install.sh`) whatever is installed on the machine is available.

## The manifest

Unknown keys are ignored (so a newer manifest still reads on an older FetchBox, unless it needs a newer
`api_version`). The exception is `permissions.uses`: it must list exactly the groups the manifest
needs (see [permissions](#permissions-and-the-trust-model)), and a group name this FetchBox does not know is an error. A manifest is at most 256 KiB. Anything invalid is reported with the reason on the plugin's card.

### Identity and platforms

| Field | |
| --- | --- |
| `api_version` | `1`. A manifest that needs a newer one is shown as *not supported*. |
| `id` | `^[a-z][a-z0-9-]{1,31}$`; the folder name; never changes. |
| `name`, `version`, `description`, `author`, `homepage`, `license` | Shown on cards. `version` is `1.2.3` or `1.2.3-rc.1`. `homepage` must be `https://`. |
| `min_fetchbox` | Lowest FetchBox version it works with. |
| `platforms` | `linux-x86_64`, `linux-aarch64`, `macos-aarch64`, `macos-x86_64`, `windows-x86_64`. Empty means any. Where the machine is not listed, the plugin is *not supported* and cannot be switched on. |
| `permissions.network` | Host patterns the extension says it talks to (`example.com`, `*.example.com`, `*`, `${setting:key}`). Shown to the user; see [permissions](#permissions-and-the-trust-model). |
| `permissions.uses` | The groups of permissions the extension uses: `network`, `programs`, `downloads`, `background`, `events`, `api`. **Required to match the rest of the manifest**, see [permissions](#permissions-and-the-trust-model). |
| `permissions.reasons` | Why it needs a group, one sentence each (`{"network": "..."}`, at most 200 characters, only for groups in `uses`). Shown to the user inside the group; translated through `i18n`. |

### Dependencies

A dependency is a program FetchBox downloads for your extension when it is switched on (or when the user presses
**Download programs**), verifies, unpacks into `<data>/plugin-data/<id>/deps/<dependency id>/` and marks executable.
You use it as `${dep:<id>}` in a command.

Two forms. **Pinned**: you give the address and the SHA-256 of the file per platform; the file can never change
without the manifest changing.

```json
{
  "id": "mytool",
  "label": "mytool 2.1",
  "platforms": {
    "linux-x86_64":  { "url": "https://example.com/mytool-2.1-linux-amd64.tar.gz", "sha256": "<64 hex digits>",
                       "archive": "tar.gz", "strip": 1, "bin": "bin/mytool", "size": 12000000 },
    "linux-aarch64": { "url": "https://example.com/mytool-2.1-linux-arm64.tar.gz", "sha256": "<64 hex digits>",
                       "archive": "tar.gz", "strip": 1, "bin": "bin/mytool" }
  },
  "bin": "bin/mytool"
}
```

**Following a GitHub repository**: FetchBox asks for the latest release (or `tag`), takes the asset named for the
platform and checks it against a checksum list that the release itself publishes (`sha256_asset`; required). This
is how the `yt-dlp` extension keeps yt-dlp current (the **Update** button fetches the newest release).

```json
{
  "id": "yt-dlp",
  "github": {
    "repo": "yt-dlp/yt-dlp",
    "assets": { "linux-x86_64": "yt-dlp_linux", "linux-aarch64": "yt-dlp_linux_aarch64", "macos-aarch64": "yt-dlp_macos" },
    "sha256_asset": "SHA2-256SUMS"
  }
}
```

Rules:

- Addresses are `https://` only and go through the same URL policy as downloads (`--block-private-urls`). A hash
  that does not match, a file larger than expected (`size` is an upper bound; the default limit is 500 MiB) or an
  archive that tries to escape its folder fails the install with the reason shown on the card.
- `archive` is `raw` (the file is the program; the default), `gz`, `zip`, `tar.gz` or `tar.xz`. For a GitHub asset
  it is taken from the file name. `strip` removes leading path components of archive entries.
- `bin` is the program's path inside what was unpacked (default: the dependency's `id`). A platform entry can have
  its own `bin` when a package names its program differently per platform (the ffmpeg of the `yt-dlp` extension
  does: it is unpacked from a wheel of the `imageio-ffmpeg` package, whose program is called after its platform).
- `optional: true` means the extension works without it; a preset can ask for it with `requires` (see
  [resolvers](#resolvers)). A missing *required* dependency keeps the extension in *preparing*.
- Choose sources you can pin: an immutable URL (a release asset, a package on PyPI with the hash that PyPI shows)
  beats "latest" in a URL.
- Prefer self-contained programs. In the Docker image the program is started directly, so a Python script or a
  PyInstaller binary that needs a shell will not work; a PyInstaller or static binary does.

### Settings

`settings` is a list of fields; FetchBox draws them in **Settings → Extensions** (or in your own tab, see
[Interface](#interface)) with a *Save* button of their own. A field:

| Key | |
| --- | --- |
| `key` | Lowercase letters, digits and `_`. |
| `type` | `text`, `secret`, `textarea`, `number`, `toggle`, `select`, `info` (a static text, from `help`), `output` (read-only; dialogs only). |
| `label`, `help`, `placeholder` | Texts. |
| `default`, `required` | `required` means the user must fill it in before it can be saved. |
| `options` | For `select`: `[{ "value": "x", "label": "X" }]`. |
| `min`, `max`, `step`, `unit` | For `number`; `unit` is shown after it (`MB/s`). |
| `show_if` | `{ "key": "mode", "equals": "custom" }`: shown only while that field has that value. |

A `secret` is never sent back to the browser (the card shows that one is saved); use it for tokens and passwords.
Text values are at most 8000 characters; only `textarea` may contain line breaks. Values reach your commands
through `${setting:key}` and your program through `initialize` and `settings_changed`.

### Templates for commands

Commands never go through a shell: every argument is one argument whatever it contains. `command` is
`${dep:<id>}`, or a path inside the plugin (`bin/tool` or `${plugin_dir}/bin/tool`); anything else (`/bin/sh`,
`../x`) is refused. Arguments are strings with these placeholders (write `$$` for a literal `$`):

| | |
| --- | --- |
| `${url}` | The address a resolver was asked about. |
| `${setting:key}` | A setting (the stored value, else its default). A number is written without a pointless `.0`, a toggle as `true`/`false`. |
| `${dep:id}` / `${dep_dir:id}` | The dependency's program / its folder. |
| `${choice.args}` | Only as a whole argument: replaced by *all* the arguments of the chosen preset. |
| `${out_dir}` | The folder a download's program must write into (resolvers' `run` only). |
| `${plugin_dir}`, `${data_dir}`, `${tmp}` | The extension's files, its data folder, its temporary folder. |

A group `{ "when": "setting:key", "args": ["--flag", "${setting:key}"] }` is only there when the setting has a
value (non-empty text, a toggle that is on, any number). `{ "when": "dep:id", ... }` is only there when that
dependency is installed:

```json
"args": ["-J", { "when": "dep:ffmpeg", "args": ["--ffmpeg-location", "${dep:ffmpeg}"] }, "--", "${url}"]
```

A placeholder that is not available where it is used (`${out_dir}` in a probe) is refused when the manifest is
installed, not when it runs.

### Resolvers

A resolver teaches FetchBox about addresses of some sites. When the user pastes an address into **Add download**,
FetchBox asks the resolvers whose `match` fits; the first that does answers instead of the ordinary look at the
address, and its answer is shown like a directory listing: one entry per item, or, for a single video, one entry
per *preset*.

```json
{
  "id": "media",
  "match": ["https://*.example.com/*", "${setting:extra_sites}"],
  "probe":  { "command": "${dep:mytool}", "args": ["--json", "--", "${url}"], "timeout_s": 60 },
  "result": {
    "items": "/entries",
    "item":   { "name": ["/title", "/id"], "url": ["/url", "/webpage_url"], "size": "/filesize" },
    "single": { "name": ["/title", "/id"], "size": ["/filesize", "/filesize_approx"] },
    "default_choice_setting": "quality"
  },
  "choices": [
    { "id": "best",  "label": "Best quality",  "category": "video", "ext": "mp4", "requires": ["ffmpeg"], "args": ["-f", "bv*+ba/b"] },
    { "id": "audio", "label": "Audio only",    "category": "music", "args": ["-f", "ba"] }
  ],
  "run": { "command": "${dep:mytool}", "args": ["${choice.args}", "-o", "${out_dir}/%(title)s.%(ext)s", "--", "${url}"] }
}
```

- **`match`**: globs on the whole address (`*` matches anything). `${setting:key}` is replaced by the lines or
  comma-separated items of that setting, which is how users add more sites.
- **`probe`**: a command that prints **JSON** on its standard output about the address (at most 8 MiB, at most
  120 s). **`result`** says where to find things in it with JSON Pointers (RFC 6901; a list of pointers means "the
  first that exists"). If the JSON has an array at `items`, the address is a list (a playlist); otherwise
  `single` describes one file. No code of yours is needed to read it. Alternatively `"process": true` asks the
  extension's [long-running program](#a-program-that-keeps-running) with `resolve` instead of running `probe`, and the
  answer is mapped the same way.
- **Without `run`**: the resolver only *finds addresses* (say, the links of an HTML page); each item's `url` is
  then downloaded by FetchBox's own engine like any other.
- **With `run`**: your program makes the file (the only way to get a video that is split into streams). The entry
  carries which resolver and preset to use (`via`), and the download is stored with the page's address; starting it
  runs `run` in a private working folder:

  | Program prints (standard output) | Meaning |
  | --- | --- |
  | `@fetchbox progress <downloaded> <total> <speed>` | Numbers in bytes and bytes/s; `NA` or `None` for unknown. Drives the bar, speed and ETA. |
  | `@fetchbox file <path>` | The finished file (inside the working folder). Without it, the biggest finished file there is taken. |
  | anything else | Ignored. The last lines of standard error become the message of a failed download. |

  Exit code `0` means done. The download takes a slot of the scheduler, shows progress, obeys the download hours
  and start times, counts in the statistics, can be paused (the program is killed with everything it started and
  **started again** on resume in the same folder, so it should continue from its partial files: `yt-dlp` does),
  cancelled and removed (the folder is deleted). The speed limits of FetchBox (the total one, the default of each
  download, a download's own) do not apply to a program that downloads by itself (the speed dialog says so; offer
  a `--limit-rate` style setting of your own if you want one), and free space is not reserved for it because the
  size is not known. The file is moved to the download folder under the name
  chosen when adding, with the real extension the program made; a name that exists gets the next free one.
- **`choices`** (presets): `label` is shown, `category` and `ext` are guesses used until the file exists, `requires`
  lists dependencies that must be installed for the preset to be offered (a preset whose dependency is missing is
  not listed; if none can run the add dialog says what to install), `args` are spliced where the command has
  `${choice.args}`. For a list, each item uses the preset named by the setting in `default_choice_setting`, else
  the first that can run.
- An error of a probe is shown as a `502` to the user with the program's last words, and never blocks adding the
  address in the ordinary way when no resolver matches.

### Hooks

A hook reacts to events. FetchBox delivers events in the background, in order, one queue per extension (256
deep: a slow hook drops the newest events instead of slowing FetchBox down), so a hook can never hold up a
download.

```json
"hooks": [
  { "on": ["download.completed", "download.failed"],
    "action": { "webhook": { "url": "${setting:url}", "headers": { "Authorization": "${setting:token}" } } } },
  { "on": ["download.progress"], "every_s": 30, "action": { "run": { "command": "bin/log-progress" } } },
  { "on": ["*"], "action": { "process": true } }
]
```

**Events** (`on` takes these names, `download.*`, `torrent.*` or `*`; progress events are only delivered to hooks
that name them: `*` and `download.*` leave them out because they come every second):
`download.added`, `.started`, `.paused`, `.resumed`, `.completed`, `.failed`, `.cancelled`, `.removed`,
`.progress`, `torrent.added`, `.started`, `.paused`, `.completed`, `.removed`, `.progress`, and `settings.changed`.
`every_s` thins progress events (default 5 s per download).

**What is delivered** is a JSON document:

```json
{ "event": "download.completed", "at": 1760000000,
  "download": { "id": 7, "url": "https://...", "filename": "file.iso", "category": "archive", "status": "completed",
                "size": 1234, "downloaded": 1234, "error": null, "...": "..." } }
```

`download.progress` has `"progress": {"downloaded", "total", "speed"}` next to `download`; `download.removed` has the
`id` and the last known `download`; torrent events have `torrent` instead of `download`; `settings.changed` has
`settings`. Passwords of downloads are never in it. The fields are the ones of the REST API
(`docs/openapi.json`, schemas `Download`, `Torrent`, `Settings`).

**Actions**:

- `webhook`: FetchBox sends the request itself (no program needed). `url` is `https://` or `http://` (a service on
  your LAN usually has no TLS) and may be `${setting:key}`; `method` defaults to `POST`; `headers` values are
  templates, and a header whose value comes out empty is left out (so an optional token costs nothing);
  `body` is `"event"` (the document above as JSON, the default) or `"template"` with `template` set to a text
  where `{{download.filename}}` (dotted path into the document) is replaced by the value (a missing path is
  empty). 10 s timeout, the URL policy applies.
- `run`: a command (see [templates](#templates-for-commands)) that gets the document on its **standard input**.
  Exit code `0` is success; failures go to the plugin's log.
- `process`: sends the document to the extension's [long-running program](#a-program-that-keeps-running) as an
  `event` notification.

### A program that keeps running

For things that need memory between events (a metrics exporter, an MCP bridge), declare a program that FetchBox
starts with the extension and keeps alive:

```json
"process": {
  "command": "${dep:exporter}",
  "args": ["--listen", "${setting:listen}"],
  "env": { "TOKEN": "${setting:token}" },
  "autostart": true,
  "restart": "on_failure"
}
```

`restart` is `on_failure` (the default), `always` or `never`. It is restarted with a back-off of 1 s doubling up to
5 min, and given up after 5 failed starts within 10 minutes (the card shows *Failed*; **Restart** tries again). A
program that does not answer three requests in a row is restarted.

The conversation is **JSON lines**: one JSON object per line on the program's standard input and output, at most
1 MiB per line. Standard error is free text: its last lines are the plugin's log (**Show log** on the card).

From FetchBox, requests (`{"id":1,"method":"...","params":{...}}`, exactly one answer each) and notifications
(no `id`, no answer):

| Method | Kind | Params |
| --- | --- | --- |
| `initialize` | request, first, 10 s to answer | `{ "api_version", "plugin_id", "fetchbox_version", "data_dir", "settings": {key: value} }`; answer with any result. |
| `resolve` | request, 60 s | `{ "url", "resolver" }`; answer with the JSON that the resolver's `result` mapping reads. Used by resolvers with `"process": true`. |
| `invoke` | request, 60 s | `{ "action", "input": {field: value} }` from an action of kind `process`; answer with an object of the [action result](#interface) or any text. |
| `event` | notification | The hook document above, for hooks with the action `process`. |
| `settings_changed` | notification | `{ "settings": {key: value} }`: the user saved the settings; no restart needed. |
| `shutdown` | request | Finish and exit; after 3 s FetchBox sends `SIGTERM` and after 3 more kills the whole process group. |

From the program, responses (`{"id":1,"result":...}` or `{"id":1,"error":{"code":-1,"message":"..."}}`) and
notifications:

| Method | Params | |
| --- | --- | --- |
| `log` | `{ "level": "info\|warning\|error", "message" }` | Goes to the plugin's log (and FetchBox's, for warnings and errors). |
| `status` | `{ "text" }` | A line shown on the card (`""` clears it). |
| `notify` | `{ "level", "message" }` | A toast shown to the user. |

Requests from the program to FetchBox (`id` and `method` from the plugin) are answered with "not implemented" for
now; the names are reserved for calls such as `downloads.add` in a future API version.

A minimal program in Python (on a host install; in Docker use a binary):

```python
#!/usr/bin/env python3
import json, sys

def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()

for line in sys.stdin:
    msg = json.loads(line)
    method, params, mid = msg.get("method"), msg.get("params", {}), msg.get("id")
    if method == "initialize":
        send({"id": mid, "result": {}})
        send({"method": "status", "params": {"text": "ready"}})
    elif method == "event":
        sys.stderr.write(f"{params['event']}\n")
    elif method == "invoke":
        send({"id": mid, "result": {"ok": True, "message": "pong"}})
    elif method == "shutdown":
        send({"id": mid, "result": {}})
        break
```

### Interface

```json
"ui": {
  "icon": "video",
  "actions": [
    { "id": "update", "label": "Update yt-dlp", "icon": "retry", "placement": "card",
      "kind": "builtin", "builtin": "update_dependencies", "confirm": "Download the newest version now?" }
  ],
  "dialogs": [ { "id": "test", "title": "Send a test", "fields": [ ... ], "buttons": [ ... ] } ],
  "tab": { "title": "My extension", "icon": "video",
           "sections": [ { "title": "Connection", "fields": ["url", "token"], "actions": ["test"] } ] }
}
```

- **`icon`**: a name from FetchBox's own set: `photo`, `music`, `video`, `program`, `archive`, `other`, `pause`,
  `play`, `retry`, `x`, `trash`, `plus`, `list`, `chart`, `sliders`, `sun`, `moon`, `menu`, `monitor`, `down`, `up`,
  `search`, `check`, `alert`, `lock`, `more`, `paste`, `layers`, `activity`, `bell`, `magnet`, `users`, `folder`,
  `puzzle` (and a few more; an unknown name falls back to `puzzle`). Extensions cannot bring their own images.
- **Actions** are buttons. `placement`: `card` (on the plugin's card in Extensions), `settings` (with its settings)
  or `hidden` (only a dialog button runs it). `confirm` asks first. `kind`:
  - `run`: runs `command` with `args` (and `timeout_s`) once. The dialog's values arrive on its **standard input**
    as a JSON object (`{"field": value}`); what it prints on standard output (at most 4000 characters) is shown
    as the message;
  - `webhook`: sends a `webhook` (as in hooks); the document for `{{...}}` templates is
    `{"plugin": "<id>", "action": "<action id>", "input": {"field": value}}`, so `{{input.message}}` is a field
    of the dialog;
  - `process`: `invoke` on the long-running program, whose answer is the result below;
  - `builtin`: `update_dependencies` (fetch the newest of every dependency) or `restart`;
  - `open_dialog`: opens the dialog named by `dialog`;
  - `open_url`: opens `url` in a new tab (`http`/`https` only).
- **Action result** (what a `process` action answers; the other kinds report success or the error themselves):
  `{ "ok": true, "message": "Sent", "level": "success|info|warning|error", "close": true,
  "open_dialog": "next", "set": { "output_field": "text" } }`. All fields are optional; `message` is shown as a toast,
  `close` closes the dialog the action came from, `set` fills `output` fields of the open dialog (how a dialog shows a
  result).
- **Dialogs** have `title`, `description`, `fields` (same as settings; values are only kept while the dialog is
  open) and `buttons`: `{ "id", "label", "style": "default|primary|danger", "action": "<action id>", "closes": false }`.
  A button without `action` just closes the dialog; with one, it runs the action with the typed values as `input`
  and (by default) leaves the dialog open unless the result says `close`. Enter submits through the primary button.
- **`tab`** gives the extension a tab of its own in **Settings** (next to *General* and *Extensions*) instead of
  its card in *Extensions*. `sections` group settings (`fields`: keys) and actions (`actions`: ids); without
  sections all settings are listed.

Everything an extension writes (labels, help, choices) is shown as **text**; never as HTML or markup.

### Translations

`i18n` maps a language code to translations whose keys are the English texts you used:

```json
"i18n": { "pl": { "Send a test message": "Wyślij wiadomość testową" } }
```

The interface looks up `i18n[<locale>][text]`, then the bare language (`pl` for `pl-PL`), then uses the text
itself. Labels, helps, placeholders, choices, dialog and button texts, the description and the confirmations are
translated. Messages that a program prints are shown as they are.

## Permissions and the trust model

**Installing an extension is giving it the right to run programs on the machine with the rights of FetchBox.**
FetchBox does not sandbox them beyond that (they run as the same operating-system user and can read the data
folder, the database included). What it does to keep that reasonable:

- Extensions are **off** when the server runs with `--no-auth` (anyone who can reach the port could install one
  otherwise) and with `--plugins off` (`FETCHBOX_PLUGINS=off`). The API for them is for signed-in users only.
- You **declare** what the extension uses in `permissions.uses`, and FetchBox holds the manifest to it: a manifest that
  runs a program but does not list `programs`, or lists `background` without a `process`, is refused when it is read.
  What each group covers:

  | Group | Covers |
  |---|---|
  | `network` | `permissions.network`, where dependencies are downloaded from, hosts that webhooks are sent to |
  | `programs` | every program it can run (dependencies, commands, the `process`) |
  | `downloads` | a resolver with `run`: its own program fetches the files |
  | `background` | a `process` that runs while the extension is on |
  | `events` | the events of its hooks |
  | `api` | `permissions.api` (reserved) |

  Add a sentence per group in `permissions.reasons` so the user knows *why*.
- Before anything is installed, FetchBox unpacks and reads the package (nothing runs) and shows **what it asks for** as
  one accordion per group (the way a phone lists permissions), with your reason and the concrete items inside: the
  programs it can run, where its dependencies come from, hosts it sends webhooks to, the events it receives. The items
  are worked out from the manifest, not from what the author claims. An update opens the groups that grew and marks
  what is new in red.
- The user approves the manifest (its SHA-256 is stored). **If `plugin.json` changes on disk** (an edit, a bad
  update), the extension is switched off and shown as *needs review* until it is approved again.
- Dependencies are verified by SHA-256 before they are used; a pinned source cannot change without the manifest
  changing.
- Programs are started without a shell, with a minimal environment and without FetchBox's secrets; every call has
  a timeout and an output limit; a crashing or flooding program cannot take FetchBox down, and downloads never
  depend on an extension being healthy (a plugin in trouble fails *its* downloads, not the others).
- `permissions.network` is **informational** for programs (FetchBox cannot stop a program from opening a
  connection); it is enforced for what FetchBox itself fetches (dependencies, webhooks, the store).

Write your manifest so that what it asks for is as little and as clear as possible, and say in `description` what
it does and what it needs.

## Testing on your machine

```sh
cargo run -p server -- --data-dir ./.dev/data --download-dir ./.dev/downloads   # or any build of FetchBox
mkdir -p ./.dev/data/plugins && cp -r hello-hook ./.dev/data/plugins/
```

1. Open **Extensions → Rescan**. A manifest that does not validate is listed with the reason.
2. **Review**, approve, switch on. A dependency downloads with a progress bar; **Show log** shows what your
   program wrote on standard error.
3. Run with `--log-activity` to see one line per event (resolving an address, a download made by an extension,
   hooks delivered).
4. Editing `plugin.json` while testing puts the extension back to *needs review*: press **Review** again (or remove
   and rescan). Editing your scripts does not.

Test a resolver without a real site: serve a small file with `python3 -m http.server`, add `http://127.0.0.1:8000/*`
to the extension's *More sites* (for `yt-dlp`) and paste `http://127.0.0.1:8000/clip.mp4`. Test a hook with a
throwaway receiver (`nc -l 9000` or a request-bin) and a download that fails or finishes quickly. Test a command by
running it yourself first: it receives exactly the arguments the template makes.

The Rust integration tests of the plugin system (`crates/plugins/tests`, `crates/server/src/tests_plugins.rs`) use a
stand-in program (`crates/test_plugin`) that behaves like a long-running plugin; look there for the exact shape of
every message.

## Packaging and the store

**A ZIP**: zip the contents of the folder (or the folder itself):

```sh
cd hello-hook && zip -r ../hello-hook-1.0.0.zip . -x '*.git*'
```

Users install it with **Extensions → Install from file**, or put the unzipped folder into `<data>/plugins/`.

**The store** in the app lists the extensions in [`official/`](official) (they ship inside the program, so it works
offline) and the entries of the catalog [`index.json`](index.json), fetched from this repository (or from
`FETCHBOX_PLUGIN_INDEX_URL` for a mirror or a fork) and cached for 6 hours. To list yours, add an entry through a
pull request:

```json
{
  "format": 1,
  "plugins": [
    {
      "id": "hello-hook",
      "name": "Hello hook",
      "version": "1.0.0",
      "description": "Posts a message to ntfy when a download finishes.",
      "author": "You",
      "homepage": "https://github.com/you/hello-hook",
      "license": "MIT",
      "icon": "bell",
      "tags": ["notifications"],
      "min_fetchbox": "0.10.0",
      "platforms": [],
      "download": { "url": "https://github.com/you/hello-hook/releases/download/v1.0.0/hello-hook-1.0.0.zip",
                    "sha256": "<64 hex digits of the ZIP>", "size": 1234 }
    }
  ]
}
```

The `download` must be an `https://` address of the ZIP and its SHA-256 (`sha256sum hello-hook-1.0.0.zip`): the
store refuses a file whose hash differs, and refuses a ZIP whose manifest is another plugin. `version` here is what
the store shows and compares with the installed one to offer **Update**; it should equal the one in the ZIP. An
entry without a usable `download` is not shown. Use a release asset (immutable) rather than a branch.

## Ideas: VictoriaMetrics and MCP

Both fit the formats above without anything new in FetchBox.

**Metrics for VictoriaMetrics (or Prometheus).** Hooks on `download.*`, `torrent.*` (and `torrent.progress`, with
`every_s: 15`) send events to a `process`: a small program (a static binary shipped as a pinned dependency) that
keeps counters and gauges (downloads by status, bytes, speeds, torrents seeding) and either serves them on a port from
a `listen` setting (VictoriaMetrics scrapes it with a `scrape_config`) or pushes them with the
[import API](https://docs.victoriametrics.com/#how-to-import-data-in-prometheus-exposition-format) to a URL from a
`push_url` setting every few seconds. Settings: `listen` or `push_url`, `token` (a `secret`), `instance` label. The
card's `status` line can show "scraped 12 s ago". It needs nothing from FetchBox but the events.

**An MCP bridge.** An MCP server is a program that speaks the Model Context Protocol; a `process` extension runs it
next to FetchBox. Version 1 has no calls from a program into FetchBox (`downloads.add` and friends are reserved
names), so the bridge talks to FetchBox's REST API with a token the user pastes into a `secret` setting
(`Authorization: Bearer`), listing and adding downloads and torrents, and uses `event` notifications to tell the
client what changed. When the reserved calls arrive, the same extension can drop the token.

## Limits

| | |
| --- | --- |
| Manifest | 256 KiB; at most 16 dependencies; lists such as hooks, actions and fields are capped at 64 items |
| ZIP | 32 MiB, 2000 files, 512 MiB unpacked, 16 levels deep |
| One-shot command (probe, action, `run` hook) | 120 s at most (30 s by default), 8 MiB of standard output, 64 KiB of standard error kept; 4 at a time overall, 2 per extension |
| Webhook | 10 s |
| Process line | 1 MiB; `initialize` 10 s; `resolve`/`invoke` 60 s |
| Hook queue | 256 events per extension; newest dropped when full |
| Setting text | 8000 characters |
| Store catalog | 1 MiB, 500 entries |
| Entries of a resolved list | 2000 |
