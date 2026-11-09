CREATE TABLE report_announcements (
    id INTEGER PRIMARY KEY,
    instrument_id INTEGER NOT NULL REFERENCES instruments(id) ON DELETE CASCADE,
    source TEXT NOT NULL,
    provider_symbol TEXT NOT NULL,
    scheduled_date TEXT NOT NULL,
    kind TEXT NOT NULL,
    fiscal_quarter_ending TEXT,
    session TEXT,
    first_seen_on TEXT NOT NULL,
    last_seen_on TEXT NOT NULL,
    baseline_period_end TEXT,
    state TEXT NOT NULL,
    resolved_period_end TEXT,
    set_aside_at TEXT,
    superseded_by INTEGER REFERENCES report_announcements(id) ON DELETE SET NULL
);
CREATE INDEX report_announcements_instrument ON report_announcements(instrument_id, state);

CREATE TABLE report_calendar_scans (
    source TEXT NOT NULL,
    calendar_date TEXT NOT NULL,
    scanned_at TEXT NOT NULL,
    scanned_on TEXT NOT NULL,
    outcome TEXT NOT NULL,
    last_success_at TEXT,
    last_success_on TEXT,
    PRIMARY KEY (source, calendar_date)
);

CREATE TABLE report_date_lookups (
    instrument_id INTEGER NOT NULL REFERENCES instruments(id) ON DELETE CASCADE,
    source TEXT NOT NULL,
    provider_symbol TEXT NOT NULL,
    last_lookup_day TEXT NOT NULL,
    last_success_on TEXT,
    last_outcome TEXT NOT NULL,
    PRIMARY KEY (instrument_id, source)
);
