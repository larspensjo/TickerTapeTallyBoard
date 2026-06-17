-- Instrument identity by ISIN. Nullable so existing Sharesight rows are untouched.
-- The partial unique index guarantees one instrument per ISIN while allowing many NULLs.
ALTER TABLE instruments ADD COLUMN isin TEXT;

CREATE UNIQUE INDEX idx_instruments_isin
    ON instruments (isin)
    WHERE isin IS NOT NULL;
