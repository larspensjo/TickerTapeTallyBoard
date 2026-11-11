use super::{Error, Table};
use chrono::{Duration, NaiveDate, Utc};
use sqlx::SqlitePool;
use ticker_tape_tally_board_backend::{
    db::{
        instruments::InstrumentRow,
        prices::{self, PriceRow},
    },
    domain::{
        opportunity_evidence::{
            lookback_series,
            parameters::{LOOKBACK_DAYS, RANGE_MIN_CLOSES, UNUSUAL_MIN_CLOSES},
        },
        PriceSnapshot,
    },
    market_data::effective_prices::{self, PriceSourceMapping},
    providers::MarketDataProvider,
};

pub const MARKETS: &[&str] = &[
    "stockholm",
    "oslo",
    "copenhagen",
    "helsinki",
    "iceland",
    "us",
    "uk",
    "germany",
    "france",
    "netherlands",
    "belgium",
    "italy",
    "spain",
    "portugal",
    "switzerland",
    "austria",
    "ireland",
    "other_europe",
    "canada",
    "australia",
    "hong_kong",
    "japan",
    "other_or_unknown",
];
pub const HISTORY_LABELS: &[&str] = &[
    "at_least_200_no_gap",
    "at_least_200_with_gap",
    "120_to_199",
    "under_120",
    "gap_over_limit",
    "split_in_lookback",
    "split_blocks_checks",
    "source_changes",
    "no_latest_price",
    "currency_excluded",
    "unusual_drop_history_ready",
    "range_history_ready",
    "top_up_needed",
    "top_up_would_fix_if_provider_supplies_year",
    "top_up_cannot_fix_nasdaq_split",
    "top_up_no_mapping",
    "top_up_cannot_fix_other_source_split",
];

pub fn listing_market(mappings: &[PriceSourceMapping], currency: &str) -> &'static str {
    if let Some(yahoo) = mappings
        .iter()
        .find(|m| m.provider == MarketDataProvider::Yahoo)
    {
        let symbol = &yahoo.provider_symbol;
        if let Some((_, suffix)) = symbol.rsplit_once('.') {
            return match suffix {
                "ST" => "stockholm",
                "OL" => "oslo",
                "CO" => "copenhagen",
                "HE" => "helsinki",
                "IC" => "iceland",
                "L" => "uk",
                "DE" | "F" | "BE" | "DU" | "HM" | "HA" | "MU" | "SG" => "germany",
                "PA" => "france",
                "AS" => "netherlands",
                "BR" => "belgium",
                "MI" => "italy",
                "MC" => "spain",
                "LS" => "portugal",
                "SW" | "VX" => "switzerland",
                "VI" => "austria",
                "IR" => "ireland",
                "AT" | "WA" | "PR" | "BD" | "IS" | "RG" | "TL" | "VS" => "other_europe",
                "TO" | "V" => "canada",
                "AX" => "australia",
                "HK" => "hong_kong",
                "T" => "japan",
                _ => "other_or_unknown",
            };
        }
        return "us";
    }
    let nordic_currency = mappings
        .iter()
        .find(|m| m.provider == MarketDataProvider::NasdaqNordic)
        .and_then(|m| m.currency.as_deref());
    let currency = nordic_currency.unwrap_or(currency);
    match currency {
        "SEK" => "stockholm",
        "NOK" => "oslo",
        "DKK" => "copenhagen",
        "EUR" if nordic_currency.is_some() => "helsinki",
        "EUR" => "other_europe",
        "ISK" => "iceland",
        "USD" => "us",
        "GBP" => "uk",
        "CHF" => "switzerland",
        "CAD" => "canada",
        "AUD" => "australia",
        "HKD" => "hong_kong",
        "JPY" => "japan",
        _ => "other_or_unknown",
    }
}

pub fn has_gap(start: NaiveDate, dates: &[NaiveDate]) -> bool {
    !lookback_series::history_gaps(start, dates).is_empty()
}

pub fn final_close(row: &PriceRow) -> Result<bool, Error> {
    let fetched = chrono::DateTime::parse_from_rfc3339(&row.fetched_at)?
        .with_timezone(&Utc)
        .date_naive();
    Ok(fetched > row.date_value()?)
}

pub async fn measure_history(
    pool: &SqlitePool,
    instrument: &InstrumentRow,
    today: NaiveDate,
    latest: Option<&PriceSnapshot>,
    mappings: &[PriceSourceMapping],
    splits: &[NaiveDate],
    table: &mut Table,
) -> Result<(), Error> {
    let start = today - Duration::days(LOOKBACK_DAYS);
    let mut rows = Vec::new();
    if let Some(latest) = latest {
        let provider = MarketDataProvider::from_db_str(latest.source.as_str())
            .ok_or("Unknown price source")?;
        rows = prices::list_for_instrument_in_range(
            pool,
            instrument.id,
            provider,
            Some(start),
            Some(today),
        )
        .await?;
    } else {
        table.add("no_latest_price");
    }
    let had_currency_mismatch = rows
        .iter()
        .any(|r| !r.currency.eq_ignore_ascii_case(&instrument.currency));
    rows.retain(|r| r.currency.eq_ignore_ascii_case(&instrument.currency));
    if had_currency_mismatch {
        table.add("currency_excluded");
    }
    let dates: Vec<_> = rows
        .iter()
        .map(PriceRow::date_value)
        .collect::<Result<_, _>>()?;
    let gap = has_gap(start, &dates);
    let unusual_short =
        lookback_series::short_history_reason(rows.len(), UNUSUAL_MIN_CLOSES).is_some();
    let range_short = lookback_series::short_history_reason(rows.len(), RANGE_MIN_CLOSES).is_some();
    table.add(match rows.len() {
        _ if !range_short && !gap => "at_least_200_no_gap",
        _ if !range_short => "at_least_200_with_gap",
        _ if !unusual_short => "120_to_199",
        _ => "under_120",
    });
    if gap {
        table.add("gap_over_limit");
    }
    let splits: Vec<_> = splits
        .iter()
        .copied()
        .filter(|s| *s >= start && *s <= today)
        .collect();
    if !splits.is_empty() {
        table.add("split_in_lookback");
    }
    let split_blocks = split_blocks(&rows, &splits)?;
    if split_blocks {
        table.add("split_blocks_checks");
    }
    let effective = effective_prices::effective_series(
        pool,
        instrument.id,
        &instrument.currency,
        Some(start),
        Some(today),
    )
    .await?;
    if effective
        .windows(2)
        .any(|pair| pair[0].source != pair[1].source)
    {
        table.add("source_changes");
    }
    if !unusual_short && lookback_series::internal_history_gaps(&dates).is_empty() && !split_blocks
    {
        table.add("unusual_drop_history_ready");
    }
    if !range_short && !gap && !split_blocks {
        table.add("range_history_ready");
    }
    if range_short || gap || split_blocks {
        table.add("top_up_needed");
        table.add(top_up_effect(
            latest.map(|p| p.source.as_str()),
            mappings,
            !splits.is_empty(),
        ));
    }
    Ok(())
}

pub fn split_blocks(rows: &[PriceRow], splits: &[NaiveDate]) -> Result<bool, Error> {
    // Preserve the inventory's lazy decoding: prices are irrelevant here, and
    // once a blocker is found later malformed timestamps cannot change its bucket.
    for split in splits {
        for row in rows {
            if !row.provider.history_is_split_adjusted_at_fetch()
                || lookback_series::split_fetch_is_stale(
                    chrono::DateTime::parse_from_rfc3339(&row.fetched_at)?
                        .with_timezone(&Utc)
                        .date_naive(),
                    *split,
                )
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// A top-up keeps the latest close's source; another mapping cannot repair its split basis.
pub fn top_up_effect(
    source: Option<&str>,
    mappings: &[PriceSourceMapping],
    has_split: bool,
) -> &'static str {
    if has_split && source == Some("NASDAQ_NORDIC") {
        return "top_up_cannot_fix_nasdaq_split";
    }
    if has_split && source.is_some_and(|s| s != "YAHOO") {
        return "top_up_cannot_fix_other_source_split";
    }
    if !mappings.iter().any(|m| Some(m.provider.as_str()) == source) {
        "top_up_no_mapping"
    } else {
        "top_up_would_fix_if_provider_supplies_year"
    }
}

pub fn dividend_source(source: Option<&str>, mappings: &[PriceSourceMapping]) -> &'static str {
    match source {
        Some("YAHOO") => "yahoo_priced",
        Some("NASDAQ_NORDIC")
            if mappings
                .iter()
                .any(|m| m.provider == MarketDataProvider::Yahoo) =>
        {
            "nasdaq_priced_with_yahoo_mapping"
        }
        Some("NASDAQ_NORDIC") => "nasdaq_only",
        Some(_) => "other_source",
        None => "no_latest_price",
    }
}
