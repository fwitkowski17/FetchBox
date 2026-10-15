-- Statistics. They must outlive the entries of the list (a download that is removed stays in the
-- numbers), so every download that ends is copied into `history`, by a trigger: whichever code
-- path ends it, it is recorded.

-- Time spent transferring, summed over pauses and restarts. Without it an average speed would
-- include the time a download sat in the queue or was paused.
ALTER TABLE downloads ADD COLUMN active_ms INTEGER NOT NULL DEFAULT 0;

CREATE TABLE history (
    download_id INTEGER PRIMARY KEY,           -- ids are AUTOINCREMENT, never reused
    added_at    INTEGER NOT NULL,
    finished_at INTEGER NOT NULL,
    outcome     TEXT    NOT NULL CHECK (outcome IN ('completed','failed','cancelled')),
    name        TEXT    NOT NULL,
    category    TEXT    NOT NULL,
    size        INTEGER,                       -- of the file, when known
    bytes       INTEGER NOT NULL,              -- fetched
    active_ms   INTEGER NOT NULL               -- 0 = not timed (older than this table)
);

CREATE INDEX idx_history_finished ON history(finished_at);

-- A download that ends again after a retry replaces its earlier row.
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

-- What happened before this table existed. Not timed, so these rows count in the totals but not
-- in the average speed.
INSERT INTO history
    (download_id, added_at, finished_at, outcome, name, category, size, bytes, active_ms)
SELECT id, created_at, COALESCE(finished_at, created_at), status, filename, category, size, downloaded, 0
  FROM downloads
 WHERE status IN ('completed','failed','cancelled');
