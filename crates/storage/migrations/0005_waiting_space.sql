-- A download that was added although its file does not fit on the disk is held back (paused)
-- instead of started; the flag lets the UI say why and show how much space is missing.
ALTER TABLE downloads ADD COLUMN waiting_space INTEGER NOT NULL DEFAULT 0;
