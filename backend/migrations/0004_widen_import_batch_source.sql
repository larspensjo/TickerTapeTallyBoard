-- no-transaction
-- Widen import_batches.source to allow 'AVANZA'. SQLite cannot alter a CHECK in
-- place, so rebuild the table. FK enforcement must be off during the swap, which
-- requires running outside a transaction (the directive above).
PRAGMA foreign_keys=OFF;

CREATE TABLE import_batches_new (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    source        TEXT NOT NULL CHECK (source IN ('SHARESIGHT', 'CSV', 'MANUAL', 'AVANZA')),
    imported_at   TEXT NOT NULL,
    raw_file_hash TEXT
);

INSERT INTO import_batches_new (id, source, imported_at, raw_file_hash)
    SELECT id, source, imported_at, raw_file_hash FROM import_batches;

DROP TABLE import_batches;

ALTER TABLE import_batches_new RENAME TO import_batches;

PRAGMA foreign_keys=ON;
