-- FTP, FTPS and SFTP sources, and logins (SPEC 6.12). SQLite cannot change a CHECK constraint,
-- so `downloads` is rebuilt again to allow the new `source_kind` values (the same procedure as in
-- 0010; the migration runner switches foreign keys off around it).
--
-- `username` and `password` are the login of the source. The password is stored as it is: the
-- download needs it again after a pause or a restart. It is never part of the API's answers and
-- never logged; the data directory is as private as the database file in it.

CREATE TABLE downloads_new (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    url         TEXT    NOT NULL,
    filename    TEXT    NOT NULL,
    category    TEXT    NOT NULL DEFAULT 'other',
    status      TEXT    NOT NULL DEFAULT 'queued'
                CHECK (status IN ('queued','downloading','paused','verifying','completed','failed','cancelled')),
    size        INTEGER,
    downloaded  INTEGER NOT NULL DEFAULT 0,
    source_kind TEXT    NOT NULL DEFAULT 'http'
                CHECK (source_kind IN ('http','ftp','sftp','magnet','torrent_file')),
    resumable   INTEGER NOT NULL DEFAULT 0,
    etag        TEXT,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch()),
    finished_at INTEGER,
    error       TEXT,
    overwrite     INTEGER NOT NULL DEFAULT 0,
    subdir        TEXT    NOT NULL DEFAULT '',
    ignore_space  INTEGER NOT NULL DEFAULT 0,
    waiting_space INTEGER NOT NULL DEFAULT 0,
    archived      INTEGER NOT NULL DEFAULT 0,
    speed_limit   INTEGER NOT NULL DEFAULT 0,
    active_ms     INTEGER NOT NULL DEFAULT 0,
    start_at      INTEGER,
    start_now     INTEGER NOT NULL DEFAULT 0,
    checksum_algo     TEXT CHECK (checksum_algo IN ('md5','sha1','sha256','sha512')),
    checksum_expected TEXT,
    checksum_actual   TEXT,
    verified      INTEGER NOT NULL DEFAULT 0,
    verify_failed INTEGER NOT NULL DEFAULT 0,
    -- The login of the source (NULL = none).
    username      TEXT,
    password      TEXT
);

INSERT INTO downloads_new
    (id, url, filename, category, status, size, downloaded, source_kind, resumable, etag,
     created_at, finished_at, error, overwrite, subdir, ignore_space, waiting_space, archived,
     speed_limit, active_ms, start_at, start_now, checksum_algo, checksum_expected,
     checksum_actual, verified, verify_failed)
SELECT id, url, filename, category, status, size, downloaded, source_kind, resumable, etag,
       created_at, finished_at, error, overwrite, subdir, ignore_space, waiting_space, archived,
       speed_limit, active_ms, start_at, start_now, checksum_algo, checksum_expected,
       checksum_actual, verified, verify_failed
  FROM downloads;

-- Ids are never reused (the history is keyed by them): carry the counter over.
DELETE FROM sqlite_sequence WHERE name = 'downloads_new';
INSERT INTO sqlite_sequence (name, seq)
SELECT 'downloads_new', seq FROM sqlite_sequence WHERE name = 'downloads';

DROP TABLE downloads;
ALTER TABLE downloads_new RENAME TO downloads;

CREATE INDEX idx_downloads_status ON downloads(status);

-- Dropping the table dropped its trigger (see 0008_stats.sql): the same one again.
CREATE TRIGGER downloads_to_history
AFTER UPDATE OF status ON downloads
WHEN NEW.status IN ('completed','failed','cancelled')
BEGIN
    INSERT OR REPLACE INTO history
        (download_id, added_at, finished_at, outcome, name, category, size, bytes, active_ms)
    VALUES
        (NEW.id, NEW.created_at, COALESCE(NEW.finished_at, unixepoch()), NEW.status,
         NEW.filename, NEW.category, NEW.size, NEW.downloaded, NEW.active_ms);
END;

-- SSH servers whose host key the user accepted. One key per host and port; a server that shows
-- another one is refused until the old row is removed.
CREATE TABLE known_hosts (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    host        TEXT    NOT NULL,
    port        INTEGER NOT NULL,
    key_type    TEXT    NOT NULL DEFAULT '',
    fingerprint TEXT    NOT NULL,
    added_at    INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (host, port)
);
