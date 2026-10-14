-- The user's explicit "replace the existing file" choice must survive restarts.
ALTER TABLE downloads ADD COLUMN overwrite INTEGER NOT NULL DEFAULT 0;
