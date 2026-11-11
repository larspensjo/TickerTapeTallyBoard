-- Rebuildable request bookkeeping; snapshots describe needs remaining after success.
CREATE TABLE history_topup_attempts (
    mapping_id INTEGER NOT NULL REFERENCES instrument_provider_symbols (id) ON DELETE CASCADE,
    attempted_on TEXT NOT NULL,
    requested_start TEXT NOT NULL,
    requested_end TEXT NOT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN ('PENDING', 'SUCCEEDED', 'FAILED')),
    earliest_returned_date TEXT,
    remaining_needs TEXT,
    PRIMARY KEY (mapping_id, attempted_on)
);
