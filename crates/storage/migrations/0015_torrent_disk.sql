-- Disks for torrents (SPEC 6.14): where a torrent is saved, chosen when it is added and kept for
-- good, like a download's disk.
--   `disk`    the folder of the download directory it is on ('' = the download directory itself);
--   `output`  the folder the engine writes it to, relative to the download directory ('' = the
--             engine's own default, `torrents/`, which is where every torrent from before disks is).
ALTER TABLE torrents ADD COLUMN disk TEXT NOT NULL DEFAULT '';
ALTER TABLE torrents ADD COLUMN output TEXT NOT NULL DEFAULT '';
