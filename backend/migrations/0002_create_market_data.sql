-- Market-data caches, provider-symbol mappings, and refresh-run metadata.
-- Decimals remain TEXT so rust_decimal can round-trip them without loss.

CREATE TABLE instrument_provider_symbols (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    instrument_id   INTEGER NOT NULL REFERENCES instruments (id),
    provider        TEXT NOT NULL CHECK (provider IN ('YAHOO', 'TWELVE_DATA', 'MANUAL')),
    provider_symbol TEXT NOT NULL,
    currency        TEXT,
    enabled         INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    UNIQUE (instrument_id, provider)
);

CREATE INDEX idx_instrument_provider_symbols_instrument
    ON instrument_provider_symbols (instrument_id);

CREATE INDEX idx_instrument_provider_symbols_provider_symbol
    ON instrument_provider_symbols (provider, provider_symbol);

CREATE TABLE prices (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    instrument_id   INTEGER NOT NULL REFERENCES instruments (id),
    provider        TEXT NOT NULL CHECK (provider IN ('YAHOO', 'TWELVE_DATA', 'MANUAL')),
    provider_symbol TEXT NOT NULL,
    date            TEXT NOT NULL,
    close           TEXT NOT NULL,
    currency        TEXT NOT NULL,
    fetched_at      TEXT NOT NULL,
    UNIQUE (instrument_id, provider, date)
);

CREATE INDEX idx_prices_instrument_date
    ON prices (instrument_id, date);

CREATE INDEX idx_prices_date
    ON prices (date);

CREATE TABLE fx_rates (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    base       TEXT NOT NULL,
    quote      TEXT NOT NULL,
    date       TEXT NOT NULL,
    rate       TEXT NOT NULL,
    provider   TEXT NOT NULL CHECK (provider IN ('FRANKFURTER', 'YAHOO', 'MANUAL')),
    fetched_at TEXT NOT NULL,
    UNIQUE (base, quote, provider, date)
);

CREATE INDEX idx_fx_rates_base_quote_date
    ON fx_rates (base, quote, date);

CREATE TABLE market_data_refresh_runs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    "trigger"   TEXT NOT NULL CHECK ("trigger" IN ('MANUAL', 'LAUNCH', 'BACKFILL')),
    started_at  TEXT NOT NULL,
    finished_at TEXT,
    status      TEXT NOT NULL CHECK (status IN ('RUNNING', 'SUCCEEDED', 'PARTIAL', 'FAILED')),
    message     TEXT,
    prices_written INTEGER NOT NULL DEFAULT 0,
    fx_rates_written INTEGER NOT NULL DEFAULT 0,
    unmapped_instruments INTEGER NOT NULL DEFAULT 0,
    failed_items INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_market_data_refresh_runs_started_at
    ON market_data_refresh_runs (started_at DESC, id DESC);
