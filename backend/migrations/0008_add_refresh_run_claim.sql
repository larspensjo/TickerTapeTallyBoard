ALTER TABLE market_data_refresh_runs ADD COLUMN claim_owner TEXT;
ALTER TABLE market_data_refresh_runs ADD COLUMN heartbeat_at TEXT;

CREATE TABLE data_revision (
    id      INTEGER PRIMARY KEY CHECK (id = 1),
    counter INTEGER NOT NULL
);

INSERT INTO data_revision (id, counter) VALUES (1, 0);
