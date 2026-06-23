-- Keep market/trade price separate from dividend cash-per-share amounts.

ALTER TABLE transactions
    ADD COLUMN dividend_per_share TEXT;

UPDATE transactions
SET dividend_per_share = price,
    price = NULL
WHERE type = 'DIVIDEND'
  AND price IS NOT NULL;
