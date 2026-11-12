-- Events share the price source's split-restated basis. Coverage belongs to a mapping.
CREATE TABLE dividend_events (
    instrument_id INTEGER NOT NULL REFERENCES instruments (id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    ex_date TEXT NOT NULL,
    amount TEXT NOT NULL,
    currency TEXT NOT NULL,
    PRIMARY KEY (instrument_id, provider, ex_date)
);

-- Valid ranges are retained when validation fails. Invalid ranges override them
-- until a later valid capture clears the covered portion. Outcomes can be extended.
CREATE TABLE dividend_coverage (
    mapping_id INTEGER NOT NULL REFERENCES instrument_provider_symbols (id) ON DELETE CASCADE,
    start_date TEXT NOT NULL,
    end_date TEXT NOT NULL,
    outcome TEXT NOT NULL,
    reason TEXT,
    PRIMARY KEY (mapping_id, start_date, end_date, outcome),
    CHECK (start_date <= end_date)
);
