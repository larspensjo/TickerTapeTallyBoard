-- Indexes are market context, never instruments or ledger holdings.
CREATE TABLE market_index_closes (
    index_code TEXT NOT NULL,
    provider TEXT NOT NULL,
    date TEXT NOT NULL,
    provider_symbol TEXT NOT NULL,
    close TEXT NOT NULL,
    currency TEXT NOT NULL,
    fetched_at TEXT NOT NULL,
    PRIMARY KEY (index_code, provider, date)
);
