-- A speed limit of its own for one download (bytes/s, 0 = none); the global limit still applies.
ALTER TABLE downloads ADD COLUMN speed_limit INTEGER NOT NULL DEFAULT 0;
