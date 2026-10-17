-- BitTorrent (SPEC 6.13). Torrents are a list of their own: a swarm has no URL, no segments and no
-- single file, so they do not share the `downloads` table.
--
-- The engine (librqbit) keeps the state of the pieces itself, in a folder next to the database;
-- this table is what FetchBox knows about a torrent whatever the engine remembers, so the list
-- survives a lost engine folder and a torrent can be added again from here:
--   `magnet`    the link a torrent was added with (none for a .torrent file);
--   `metainfo`  the .torrent file (the bytes the user sent, or what the swarm sent for a magnet link
--               once it arrived), so a restart does not have to ask the swarm for it again;
--   `paused`    what the user last chose (the engine's own flag is the one that counts while it runs);
--   `uploaded`  bytes sent over all runs, as the engine only counts since it started.
CREATE TABLE torrents (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    info_hash   TEXT    NOT NULL UNIQUE CHECK (length(info_hash) = 40),
    name        TEXT    NOT NULL,
    magnet      TEXT,
    metainfo    BLOB,
    size        INTEGER,
    file_count  INTEGER NOT NULL DEFAULT 0,
    paused      INTEGER NOT NULL DEFAULT 0,
    uploaded    INTEGER NOT NULL DEFAULT 0,
    added_at    INTEGER NOT NULL DEFAULT (unixepoch()),
    finished_at INTEGER,
    error       TEXT
);
