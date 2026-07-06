-- Conviction is user-managed portfolio metadata, one level per instrument.
-- It is not imported and is never reset by import refresh, rollback, or ledger
-- edits. Every instrument gets an explicit default of OTHER; existing rows are
-- backfilled to OTHER by the NOT NULL DEFAULT.
ALTER TABLE instruments
    ADD COLUMN conviction TEXT NOT NULL DEFAULT 'OTHER'
        CHECK (conviction IN ('OTHER', 'LOW', 'MEDIUM', 'HIGH'));
