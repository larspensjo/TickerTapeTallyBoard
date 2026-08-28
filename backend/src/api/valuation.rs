use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::sqlite::SqlitePool;

use crate::api::error::ApiError;
use crate::db::{fx_rates, instruments};
use crate::domain::{Availability, DataFreshness, FxCandidate, PriceCandidate, ValuationReason};
use crate::market_data::effective_prices;
use crate::providers::BASE_FX_PROVIDER;

pub(super) const BASE_CURRENCY: &str = "SEK";

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum AvailabilityResponse {
    Available { value: String },
    Unavailable { reasons: Vec<String> },
}

#[derive(Debug, Serialize)]
pub(crate) struct PriceSnapshotResponse {
    pub date: String,
    pub close: String,
    pub currency: String,
    pub source: String,
    pub freshness: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct FxSnapshotResponse {
    pub date: String,
    pub rate: String,
    pub base: String,
    pub quote: String,
    pub freshness: String,
}

pub(super) struct ValuationInputs {
    pub has_price_coverage: bool,
    pub latest_price: Option<PriceCandidate>,
    pub previous_price: Option<PriceCandidate>,
    pub latest_fx: Option<FxCandidate>,
    pub previous_fx: Option<FxCandidate>,
}

pub(super) struct PeriodInputs {
    pub start_price: Option<PriceCandidate>,
    pub end_price: Option<PriceCandidate>,
    pub start_fx: Option<FxCandidate>,
    pub end_fx: Option<FxCandidate>,
}

pub(super) async fn load_valuation_inputs(
    pool: &SqlitePool,
    instrument: &instruments::InstrumentRow,
    valuation_date: NaiveDate,
) -> Result<ValuationInputs, ApiError> {
    let has_price_coverage = effective_prices::has_price_coverage(pool, instrument.id).await?;

    let (latest_price, previous_price) = if has_price_coverage {
        let latest =
            effective_prices::effective_latest_on_or_before(pool, instrument.id, valuation_date)
                .await?;

        let previous = if let Some(ref latest) = latest {
            effective_prices::effective_previous_before(pool, instrument.id, latest.date).await?
        } else {
            None
        };

        (latest, previous)
    } else {
        (None, None)
    };

    let (latest_fx, previous_fx) = if instrument.currency.eq_ignore_ascii_case(BASE_CURRENCY) {
        (None, None)
    } else {
        let latest = fx_rates::find_latest_on_or_before(
            pool,
            &instrument.currency,
            BASE_CURRENCY,
            BASE_FX_PROVIDER,
            valuation_date,
        )
        .await?
        .and_then(fx_candidate);

        let previous = if let Some(ref latest) = latest {
            fx_rates::find_previous_before(
                pool,
                &instrument.currency,
                BASE_CURRENCY,
                BASE_FX_PROVIDER,
                latest.date,
            )
            .await?
            .and_then(fx_candidate)
        } else {
            None
        };

        (latest, previous)
    };

    Ok(ValuationInputs {
        has_price_coverage,
        latest_price,
        previous_price,
        latest_fx,
        previous_fx,
    })
}

pub(super) async fn load_period_inputs(
    pool: &SqlitePool,
    instrument: &instruments::InstrumentRow,
    start_date: Option<NaiveDate>,
    end_date: NaiveDate,
) -> Result<PeriodInputs, ApiError> {
    let has_price_coverage = effective_prices::has_price_coverage(pool, instrument.id).await?;

    let (start_price, end_price) = if has_price_coverage {
        let end =
            effective_prices::effective_latest_on_or_before(pool, instrument.id, end_date).await?;
        let start = if let Some(sd) = start_date {
            effective_prices::effective_latest_on_or_before(pool, instrument.id, sd).await?
        } else {
            None
        };
        (start, end)
    } else {
        (None, None)
    };

    let is_base_currency = instrument.currency.eq_ignore_ascii_case(BASE_CURRENCY);
    let (start_fx, end_fx) = if is_base_currency {
        (None, None)
    } else {
        let end = fx_rates::find_latest_on_or_before(
            pool,
            &instrument.currency,
            BASE_CURRENCY,
            BASE_FX_PROVIDER,
            end_date,
        )
        .await?
        .and_then(fx_candidate);
        let start = if let Some(sd) = start_date {
            fx_rates::find_latest_on_or_before(
                pool,
                &instrument.currency,
                BASE_CURRENCY,
                BASE_FX_PROVIDER,
                sd,
            )
            .await?
            .and_then(fx_candidate)
        } else {
            None
        };
        (start, end)
    };

    Ok(PeriodInputs {
        start_price,
        end_price,
        start_fx,
        end_fx,
    })
}

pub(super) fn serialize_availability<T, F>(value: &Availability<T>, f: F) -> AvailabilityResponse
where
    F: Fn(&T) -> String,
{
    match value {
        Availability::Available(value) => AvailabilityResponse::Available { value: f(value) },
        Availability::Unavailable { reasons } => AvailabilityResponse::Unavailable {
            reasons: reasons.iter().map(serialize_valuation_reason).collect(),
        },
    }
}

pub(super) fn serialize_valuation_reason(reason: &ValuationReason) -> String {
    match reason {
        ValuationReason::MissingPrice => "missing_price".to_string(),
        ValuationReason::MissingFx => "missing_fx".to_string(),
        ValuationReason::MissingPreviousClose => "missing_previous_close".to_string(),
        ValuationReason::PreviousCloseSourceMismatch => {
            "previous_close_source_mismatch".to_string()
        }
        ValuationReason::MissingPreviousFx => "missing_previous_fx".to_string(),
        ValuationReason::StalePrice { trading_days } => {
            format!("stale_price_{}_days", trading_days)
        }
        ValuationReason::StaleFx { trading_days } => {
            format!("stale_fx_{}_days", trading_days)
        }
        ValuationReason::ZeroCostBasis => "zero_cost_basis".to_string(),
        ValuationReason::ZeroPreviousMarketValue => "zero_previous_market_value".to_string(),
        ValuationReason::BaseCostBasisUnavailable { .. } => {
            "base_cost_basis_unavailable".to_string()
        }
        ValuationReason::MissingStartPrice => "missing_start_price".to_string(),
        ValuationReason::MissingEndPrice => "missing_end_price".to_string(),
        ValuationReason::MissingStartFx => "missing_start_fx".to_string(),
        ValuationReason::MissingEndFx => "missing_end_fx".to_string(),
        ValuationReason::MissingTransactionPrice { transaction_id } => {
            format!("missing_transaction_price_{transaction_id}")
        }
        ValuationReason::MissingTransactionFx { transaction_id } => {
            format!("missing_transaction_fx_{transaction_id}")
        }
        ValuationReason::ZeroOrInvalidPerformanceDenominator => {
            "zero_or_invalid_performance_denominator".to_string()
        }
        ValuationReason::PerformanceDidNotConverge => "performance_did_not_converge".to_string(),
    }
}

pub(super) fn price_snapshot_response(
    snapshot: &crate::domain::PriceSnapshot,
) -> PriceSnapshotResponse {
    PriceSnapshotResponse {
        date: snapshot.date.format("%Y-%m-%d").to_string(),
        close: money_string(snapshot.close),
        currency: snapshot.currency.clone(),
        source: snapshot.source.as_str().to_owned(),
        freshness: serialize_freshness(snapshot.freshness),
    }
}

pub(super) fn fx_snapshot_response(snapshot: &crate::domain::FxSnapshot) -> FxSnapshotResponse {
    FxSnapshotResponse {
        date: snapshot.date.format("%Y-%m-%d").to_string(),
        rate: snapshot.rate.to_string(),
        base: snapshot.base.clone(),
        quote: snapshot.quote.clone(),
        freshness: serialize_freshness(snapshot.freshness),
    }
}

pub(super) fn money_string(value: Decimal) -> String {
    let rounded = value.round_dp(2);
    if rounded.is_zero() {
        return "0.00".to_owned();
    }

    let raw = rounded.to_string();
    match raw.split_once('.') {
        Some((whole, fractional)) => {
            let two_digits = match fractional.len() {
                0 => "00".to_owned(),
                1 => format!("{fractional}0"),
                _ => fractional[..2].to_owned(),
            };
            format!("{whole}.{two_digits}")
        }
        None => format!("{raw}.00"),
    }
}

fn fx_candidate(row: fx_rates::FxRateRow) -> Option<FxCandidate> {
    let date = row.date_value().ok()?;
    let rate = row.rate_decimal().ok()?;
    Some(FxCandidate {
        date,
        rate,
        base: row.base,
        quote: row.quote,
    })
}

pub(super) fn staler_freshness(left: DataFreshness, right: DataFreshness) -> DataFreshness {
    fn rank(freshness: DataFreshness) -> (u8, i64) {
        match freshness {
            DataFreshness::Fresh => (0, 0),
            DataFreshness::MinorStale { trading_days } => (1, trading_days),
            DataFreshness::WarningStale { trading_days } => (2, trading_days),
        }
    }

    if rank(left) >= rank(right) {
        left
    } else {
        right
    }
}

pub(super) fn serialize_freshness(freshness: DataFreshness) -> String {
    match freshness {
        DataFreshness::Fresh => "fresh".to_string(),
        DataFreshness::MinorStale { trading_days } => {
            format!("minor_stale_{}_days", trading_days)
        }
        DataFreshness::WarningStale { trading_days } => {
            format!("warning_stale_{}_days", trading_days)
        }
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use super::{money_string, serialize_valuation_reason};
    use crate::domain::ValuationReason;

    #[test]
    fn money_string_formats_two_decimals_and_normalizes_negative_zero() {
        assert_eq!(money_string(dec!(12.3)), "12.30");
        assert_eq!(money_string(dec!(-0.001)), "0.00");
    }

    #[test]
    fn serializes_previous_close_source_mismatch() {
        assert_eq!(
            serialize_valuation_reason(&ValuationReason::PreviousCloseSourceMismatch),
            "previous_close_source_mismatch"
        );
    }
}
