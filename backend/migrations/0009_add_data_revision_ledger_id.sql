CREATE TABLE data_revision_with_ledger_id (
    id        INTEGER PRIMARY KEY CHECK (id = 1),
    ledger_id TEXT NOT NULL,
    counter   INTEGER NOT NULL
);

INSERT INTO data_revision_with_ledger_id (id, ledger_id, counter)
SELECT id, lower(hex(randomblob(16))), counter FROM data_revision;

DROP TABLE data_revision;

ALTER TABLE data_revision_with_ledger_id RENAME TO data_revision;
