CREATE TABLE instrument_fundamentals_sources (
    instrument_id INTEGER PRIMARY KEY REFERENCES instruments(id),
    source TEXT,
    provider_symbol TEXT,
    security_type TEXT,
    classification TEXT NOT NULL,
    origin TEXT NOT NULL,
    status TEXT NOT NULL,
    candidates TEXT NOT NULL DEFAULT '[]',
    generation INTEGER NOT NULL,
    last_attempt_at TEXT,
    last_attempt_status TEXT,
    last_attempt_reason TEXT,
    last_success_at TEXT,
    consecutive_empty INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE fundamental_values (
    id INTEGER PRIMARY KEY,
    instrument_id INTEGER NOT NULL REFERENCES instruments(id),
    source TEXT NOT NULL,
    provider_symbol TEXT NOT NULL,
    period_type TEXT NOT NULL,
    period_end TEXT NOT NULL,
    metric TEXT NOT NULL,
    value TEXT NOT NULL,
    currency TEXT,
    first_seen_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    set_aside_at TEXT,
    set_aside_reason TEXT
);
CREATE UNIQUE INDEX fundamental_values_active_identity ON fundamental_values
    (instrument_id, source, provider_symbol, period_type, period_end, metric)
    WHERE set_aside_at IS NULL;

CREATE TABLE fundamental_period_observations (
    id INTEGER PRIMARY KEY,
    instrument_id INTEGER NOT NULL REFERENCES instruments(id),
    period_type TEXT NOT NULL,
    period_end TEXT NOT NULL,
    provider_symbol TEXT NOT NULL,
    first_seen_on TEXT NOT NULL,
    absent_fetch_on TEXT,
    set_aside_at TEXT,
    set_aside_reason TEXT
);
CREATE UNIQUE INDEX fundamental_observations_active_identity ON fundamental_period_observations
    (instrument_id, period_type, period_end, provider_symbol) WHERE set_aside_at IS NULL;

CREATE TABLE fundamental_split_events (
    id INTEGER PRIMARY KEY,
    instrument_id INTEGER NOT NULL REFERENCES instruments(id),
    provider_symbol TEXT NOT NULL,
    split_date TEXT NOT NULL,
    numerator TEXT NOT NULL,
    denominator TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    set_aside_at TEXT,
    set_aside_reason TEXT
);
CREATE UNIQUE INDEX fundamental_splits_active_identity ON fundamental_split_events
    (instrument_id, provider_symbol, split_date) WHERE set_aside_at IS NULL;

ALTER TABLE market_data_refresh_runs ADD COLUMN fundamentals_status TEXT NOT NULL DEFAULT 'not_run';
ALTER TABLE market_data_refresh_runs ADD COLUMN fundamentals_fetched INTEGER NOT NULL DEFAULT 0;
ALTER TABLE market_data_refresh_runs ADD COLUMN fundamentals_skipped INTEGER NOT NULL DEFAULT 0;
ALTER TABLE market_data_refresh_runs ADD COLUMN fundamentals_superseded INTEGER NOT NULL DEFAULT 0;
ALTER TABLE market_data_refresh_runs ADD COLUMN fundamentals_failed INTEGER NOT NULL DEFAULT 0;
