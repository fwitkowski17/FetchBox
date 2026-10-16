-- Download hours (SPEC 6.3.2): a download may be set to begin at a moment (`start_at`, unix
-- seconds) or to ignore the global download hours (`start_now`).
ALTER TABLE downloads ADD COLUMN start_at INTEGER;
ALTER TABLE downloads ADD COLUMN start_now INTEGER NOT NULL DEFAULT 0;
