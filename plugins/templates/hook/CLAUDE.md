# __NAME__ (FetchBox extension `__ID__`)

A FetchBox extension in a repository of its own. It has no program: everything is in `plugin.json`, which FetchBox reads and
draws (settings, a dialog, a button) and runs (the hooks send webhooks itself). The guide to everything a manifest can do is
`plugins/README.md` in the FetchBox repository (https://github.com/fwitkowski17/FetchBox); read the Hooks and Interface
sections before changing the manifest.

## Commands

- Check: `jq empty plugin.json` (what `ci.yml` runs). FetchBox validates the rest when it reads the manifest, so try it:
- Try it in a FetchBox: `fetchbox --data-dir <data dir of that FetchBox> plugin link .` (from a FetchBox checkout:
  `cargo plugin-link <this folder>`). Then Extensions -> Rescan; a manifest that does not validate is listed with the reason.
  Review, approve, switch on, fill in the settings. After every change to `plugin.json` run `plugin link` again and approve again.

## Rules that are easy to break

- `permissions.uses` must list exactly the groups the manifest really uses (`network`, `programs`, `background`, `events`,
  `downloads`, `api`), each with a `reasons` sentence of at most 200 characters, or FetchBox refuses the manifest. Webhooks
  need `network`, hooks need `events`; a command or a dependency adds `programs`.
- Any change to `plugin.json` puts an installed extension back to "needs review" (the user approves the new hash).
- Templates: `${setting:key}` in values and headers (a header whose value is empty is left out), `{{download.filename}}` (a path
  into the event document) in a `template` body. Events are `download.completed`, `download.failed`, `torrent.*` and the other
  names in the guide; `*` and `download.*` leave progress events out.
- Keys of settings, actions, dialogs and fields start with a lowercase letter and have lowercase letters, digits, `_` and `-`
  (at most 32 characters); every action a dialog button or `ui.tab` names must be declared.
- Texts shown to the user are plain text, never HTML; translations go in `i18n`, keyed by the English text.
- The version in `plugin.json` is the release: the tag `v<version>` must equal it.

## Releasing

Set the real repository in `homepage` (it is `https://github.com/__REPO__` now), bump `version`, commit, then
`git tag v<version> && git push origin v<version>`. `release.yml` publishes `__ID__-<version>.zip`. Details in README.md.
