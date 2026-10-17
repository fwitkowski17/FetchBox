-- Torrent hours (SPEC 6.3.2): a torrent can opt out of them ("Start now"), like a download.
ALTER TABLE torrents ADD COLUMN start_now INTEGER NOT NULL DEFAULT 0;
