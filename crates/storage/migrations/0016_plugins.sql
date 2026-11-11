-- Plugins (SPEC 6.15).
--   `plugins`          the installed ones. The files live in <data dir>/plugins/<id>/ and are the source
--                      of truth for what a plugin is; this row says whether it is switched on, where it
--                      came from and which manifest the user approved (a SHA-256 of `plugin.json`: a
--                      manifest that differs from it does not run until it is approved again).
--   `plugin_settings`  the values of a plugin's settings, as JSON scalars. Secrets are in here too and
--                      are never returned by the API.
-- A download whose file is made by a plugin's own program instead of FetchBox's engine (a "runner")
-- remembers which one: `runner` is '<plugin>/<resolver>', `runner_ref` the id of the chosen preset.
-- Both are NULL for every ordinary download.
CREATE TABLE plugins (
    id            TEXT PRIMARY KEY,
    version       TEXT NOT NULL,
    enabled       INTEGER NOT NULL DEFAULT 0,
    source        TEXT NOT NULL CHECK (source IN ('bundled', 'store', 'zip', 'folder')),
    approved_hash TEXT NOT NULL,
    store_sha256  TEXT,
    installed_at  INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at    INTEGER
);

CREATE TABLE plugin_settings (
    plugin_id TEXT NOT NULL REFERENCES plugins(id) ON DELETE CASCADE,
    key       TEXT NOT NULL,
    value     TEXT NOT NULL,
    PRIMARY KEY (plugin_id, key)
);

ALTER TABLE downloads ADD COLUMN runner TEXT;
ALTER TABLE downloads ADD COLUMN runner_ref TEXT;
