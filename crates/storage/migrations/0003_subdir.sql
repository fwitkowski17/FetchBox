-- Where the file goes, decided when the download is added (category subdirectories).
ALTER TABLE downloads ADD COLUMN subdir TEXT NOT NULL DEFAULT '';
