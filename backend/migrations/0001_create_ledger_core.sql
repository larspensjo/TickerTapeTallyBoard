-- Ledger core: instruments, import batches, and the transaction ledger.
-- Decimals are stored as TEXT (rust_decimal string round-trip); dates as ISO-8601 TEXT.

CREATE TABLE instruments (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    symbol   TEXT NOT NULL,
    exchange TEXT NOT NULL,
    name     TEXT NOT NULL,
    type     TEXT NOT NULL CHECK (type IN ('STOCK', 'ETF', 'FUND')),
    currency TEXT NOT NULL,
    UNIQUE (exchange, symbol)
);

CREATE TABLE import_batches (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    source        TEXT NOT NULL CHECK (source IN ('SHARESIGHT', 'CSV', 'MANUAL')),
    imported_at   TEXT NOT NULL,
    raw_file_hash TEXT
);

CREATE TABLE transactions (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    instrument_id      INTEGER NOT NULL REFERENCES instruments (id),
    type               TEXT NOT NULL CHECK (type IN ('BUY', 'SELL', 'SPLIT', 'DIVIDEND')),
    trade_date         TEXT NOT NULL, -- ISO-8601 YYYY-MM-DD.
    quantity           INTEGER NOT NULL, -- Signed position effect: buy > 0, sell < 0, split delta.
    price              TEXT,
    currency           TEXT,
    fx_rate_to_base    TEXT,
    brokerage          TEXT,
    brokerage_currency TEXT,
    source_value       TEXT,
    source_currency    TEXT,
    note               TEXT,
    import_batch_id    INTEGER REFERENCES import_batches (id)
);

CREATE INDEX idx_transactions_instrument
    ON transactions (instrument_id, trade_date, id);
