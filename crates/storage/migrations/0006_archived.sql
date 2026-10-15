-- "Archived": a finished download hidden from the main list (the file stays where it is).
ALTER TABLE downloads ADD COLUMN archived INTEGER NOT NULL DEFAULT 0;
