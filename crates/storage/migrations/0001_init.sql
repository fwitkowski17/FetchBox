CREATE TABLE downloads (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    url         TEXT    NOT NULL,
    filename    TEXT    NOT NULL,
    category    TEXT    NOT NULL DEFAULT 'other',
    status      TEXT    NOT NULL DEFAULT 'queued'
                CHECK (status IN ('queued','downloading','paused','completed','failed','cancelled')),
    size        INTEGER,
    downloaded  INTEGER NOT NULL DEFAULT 0,
    source_kind TEXT    NOT NULL DEFAULT 'http'
                CHECK (source_kind IN ('http','magnet','torrent_file')),
    resumable   INTEGER NOT NULL DEFAULT 0,
    etag        TEXT,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch()),
    finished_at INTEGER,
    error       TEXT
);

CREATE INDEX idx_downloads_status ON downloads(status);

CREATE TABLE segments (
    download_id INTEGER NOT NULL REFERENCES downloads(id) ON DELETE CASCADE,
    idx         INTEGER NOT NULL,
    start       INTEGER NOT NULL,
    end         INTEGER NOT NULL,
    downloaded  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (download_id, idx)
);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
