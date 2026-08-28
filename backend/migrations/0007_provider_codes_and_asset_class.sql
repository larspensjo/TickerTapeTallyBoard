-- no-transaction
-- SQLite cannot remove provider CHECK constraints or add asset_class in place.
-- Rebuild these tables so enum-typed Rust repository paths, rather than
-- duplicated database provider lists, enforce the provider invariant.
PRAGMA foreign_keys=OFF;

CREATE TABLE instrument_provider_symbols_new (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    instrument_id   INTEGER NOT NULL REFERENCES instruments (id),
    provider        TEXT NOT NULL,
    provider_symbol TEXT NOT NULL,
    asset_class     TEXT,
    currency        TEXT,
    enabled         INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    UNIQUE (instrument_id, provider)
);

INSERT INTO instrument_provider_symbols_new
    (id, instrument_id, provider, provider_symbol, currency, enabled, created_at, updated_at)
SELECT id, instrument_id, provider, provider_symbol, currency, enabled, created_at, updated_at
FROM instrument_provider_symbols;

DROP TABLE instrument_provider_symbols;
ALTER TABLE instrument_provider_symbols_new RENAME TO instrument_provider_symbols;

CREATE INDEX idx_instrument_provider_symbols_instrument
    ON instrument_provider_symbols (instrument_id);
CREATE INDEX idx_instrument_provider_symbols_provider_symbol
    ON instrument_provider_symbols (provider, provider_symbol);

CREATE TABLE prices_new (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    instrument_id   INTEGER NOT NULL REFERENCES instruments (id),
    provider        TEXT NOT NULL,
    provider_symbol TEXT NOT NULL,
    date            TEXT NOT NULL,
    close           TEXT NOT NULL,
    currency        TEXT NOT NULL,
    fetched_at      TEXT NOT NULL,
    UNIQUE (instrument_id, provider, date)
);

INSERT INTO prices_new
    (id, instrument_id, provider, provider_symbol, date, close, currency, fetched_at)
SELECT id, instrument_id, provider, provider_symbol, date, close, currency, fetched_at
FROM prices;

DROP TABLE prices;
ALTER TABLE prices_new RENAME TO prices;

CREATE INDEX idx_prices_instrument_date ON prices (instrument_id, date);
CREATE INDEX idx_prices_date ON prices (date);

CREATE TABLE fx_rates_new (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    base       TEXT NOT NULL,
    quote      TEXT NOT NULL,
    date       TEXT NOT NULL,
    rate       TEXT NOT NULL,
    provider   TEXT NOT NULL,
    fetched_at TEXT NOT NULL,
    UNIQUE (base, quote, provider, date)
);

INSERT INTO fx_rates_new (id, base, quote, date, rate, provider, fetched_at)
SELECT id, base, quote, date, rate, provider, fetched_at FROM fx_rates;

DROP TABLE fx_rates;
ALTER TABLE fx_rates_new RENAME TO fx_rates;

CREATE INDEX idx_fx_rates_base_quote_date ON fx_rates (base, quote, date);

PRAGMA foreign_keys=ON;
