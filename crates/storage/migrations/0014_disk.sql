-- Disks (SPEC 6.14): the folder of the download directory a download is saved on, chosen when it
-- is added and kept for good, so changing the default disk later leaves existing entries (and
-- their files) where they are. Empty = the download directory itself, which is what every row
-- from before this column means.
ALTER TABLE downloads ADD COLUMN disk TEXT NOT NULL DEFAULT '';
