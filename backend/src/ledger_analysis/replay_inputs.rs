//! Read-only loading boundary for the offline replay. Errors contain no ledger data.
use crate::{
    db::{self, prices},
    domain::{
        self,
        opportunity_evidence::{lookback_series::HistoryClose, replay_rules::is_final},
        LedgerTransaction, PriceCandidate, ProviderCode, TransactionKind,
    },
    market_data::effective_prices,
};
use chrono::{DateTime, NaiveDate, Utc};
use sqlx::SqlitePool;
use std::collections::BTreeSet;

#[derive(Clone)]
pub struct ReplayInstrument {
    pub key: i64,
    pub split_adjusted_sources: BTreeSet<String>,
    pub currency: String,
    pub ledger: Vec<LedgerTransaction>,
    /// All stored enabled-source rows, provider-precedence then date order.
    pub stored: Vec<HistoryClose>,
    /// One effective stored bar per date, before eligibility filtering.
    pub sessions: Vec<HistoryClose>,
    /// The effective price series after non-final bars are removed.
    pub finals: Vec<HistoryClose>,
    pub ledger_splits: Vec<NaiveDate>,
    pub provider_splits: Vec<NaiveDate>,
    /// Observed quarterly EPS pairs only; these are NOT point-in-time figures.
    pub observed_eps: Vec<(NaiveDate, NaiveDate)>,
}

pub fn resolve_closes(stored: &[HistoryClose], final_only: bool) -> Vec<HistoryClose> {
    // Each row is a precedence slot. Existing domain resolution owns date ties.
    let slots: Vec<Vec<_>> = stored
        .iter()
        .filter(|c| !final_only || is_final(c))
        .map(|c| {
            vec![PriceCandidate {
                date: c.date,
                close: c.close,
                currency: c.currency.clone(),
                source: c.provider.clone(),
            }]
        })
        .collect();
    let slices: Vec<_> = slots.iter().map(Vec::as_slice).collect();
    domain::resolve_price_series(&slices)
        .into_iter()
        .filter_map(|c| {
            stored
                .iter()
                .find(|row| {
                    row.date == c.date && row.provider == c.source && (!final_only || is_final(row))
                })
                .cloned()
        })
        .collect()
}

pub async fn load(
    pool: &SqlitePool,
    as_of: NaiveDate,
) -> Result<Vec<ReplayInstrument>, &'static str> {
    let instruments = db::instruments::list(pool)
        .await
        .map_err(|_| "Replay input inventory failed.")?;
    let mut out = Vec::new();
    for instrument in instruments {
        let mut ledger = db::transactions::ledger_for_instrument(pool, instrument.id)
            .await
            .map_err(|_| "Replay ledger loading failed.")?;
        ledger.retain(|t| t.trade_date <= as_of);
        if !ledger.iter().any(|t| t.kind == TransactionKind::Buy) {
            continue;
        }
        let mut stored = Vec::new();
        let mut split_adjusted_sources = BTreeSet::new();
        for mapping in effective_prices::enabled_price_sources(pool, instrument.id)
            .await
            .map_err(|_| "Replay source loading failed.")?
        {
            if mapping.provider.history_is_split_adjusted_at_fetch() {
                split_adjusted_sources.insert(mapping.provider.as_str().to_owned());
            }
            let rows = prices::list_for_instrument_in_range(
                pool,
                instrument.id,
                mapping.provider,
                None,
                Some(as_of),
            )
            .await
            .map_err(|_| "Replay close loading failed.")?;
            for row in rows {
                stored.push(HistoryClose {
                    date: row
                        .date_value()
                        .map_err(|_| "Replay close date decoding failed.")?,
                    close: row
                        .close_decimal()
                        .map_err(|_| "Replay close decoding failed.")?,
                    currency: row.currency,
                    provider: ProviderCode::new(row.provider.as_str()),
                    fetched_at: DateTime::parse_from_rfc3339(&row.fetched_at)
                        .map_err(|_| "Replay fetch date decoding failed.")?
                        .with_timezone(&Utc),
                });
            }
        }
        let ledger_splits = ledger
            .iter()
            .filter(|t| t.kind == TransactionKind::Split)
            .map(|t| t.trade_date)
            .collect();
        let provider_splits = db::fundamental_values::active_splits(pool, instrument.id)
            .await
            .map_err(|_| "Replay split loading failed.")?
            .into_iter()
            .map(|s| s.date)
            .filter(|d| *d <= as_of)
            .collect();
        let mut observed_eps = Vec::new();
        for value in db::fundamental_values::active_values(pool, instrument.id)
            .await
            .map_err(|_| "Replay company observation loading failed.")?
        {
            if value.input.period_type == domain::fundamentals::PeriodType::Quarter
                && value.input.metric == domain::fundamentals::Metric::DilutedEps
            {
                let observed = DateTime::parse_from_rfc3339(&value.first_seen_at)
                    .map_err(|_| "Replay company observation date decoding failed.")?
                    .with_timezone(&Utc)
                    .date_naive();
                observed_eps.push((value.input.period_end, observed));
            }
        }
        out.push(ReplayInstrument {
            key: instrument.id,
            split_adjusted_sources,
            currency: instrument.currency,
            sessions: resolve_closes(&stored, false),
            finals: resolve_closes(&stored, true),
            stored,
            ledger,
            ledger_splits,
            provider_splits,
            observed_eps,
        });
    }
    Ok(out)
}
