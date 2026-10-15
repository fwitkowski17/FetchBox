-- The user's explicit "add it even though it does not fit" choice must survive restarts: the
-- engine skips its free-space check for such a download.
ALTER TABLE downloads ADD COLUMN ignore_space INTEGER NOT NULL DEFAULT 0;
