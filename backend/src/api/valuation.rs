use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::sqlite::SqlitePool;

use crate::api::error::ApiError;
use crate::db::{fx_rates, instruments, prices, provider_symbols};
use crate::domain::{Availability, DataFreshness, FxCandidate, PriceCandidate, ValuationReason};

pub(super) const BASE_CURRENCY: &str = "SEK";
pub(super) const PRICE_PROVIDER: &str = "YAHOO";
pub(super) const FX_PROVIDER: &str = "FRANKFURTER";

#[derive(Debug, Serialize)]
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
    pub price_mapping_enabled: bool,
    pub latest_price: Option<PriceCandidate>,
    pub previous_price: Option<PriceCandidate>,
    pub latest_fx: Option<FxCandidate>,
    pub previous_fx: Option<FxCandidate>,
}

pub(super) async fn load_valuation_inputs(
    pool: &SqlitePool,
    instrument: &instruments::InstrumentRow,
    valuation_date: NaiveDate,
) -> Result<ValuationInputs, ApiError> {
    let price_mapping =
        provider_symbols::find_by_instrument_provider(pool, instrument.id, PRICE_PROVIDER).await?;
    let price_mapping_enabled = price_mapping.as_ref().is_some_and(|row| row.enabled);

    let (latest_price, previous_price) = if price_mapping_enabled {
        let latest =
            prices::find_latest_on_or_before(pool, instrument.id, PRICE_PROVIDER, valuation_date)
                .await?
                .and_then(price_candidate);

        let previous = if let Some(ref latest) = latest {
            prices::find_previous_before(pool, instrument.id, PRICE_PROVIDER, latest.date)
                .await?
                .and_then(price_candidate)
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
            FX_PROVIDER,
            valuation_date,
        )
        .await?
        .and_then(fx_candidate);

        let previous = if let Some(ref latest) = latest {
            fx_rates::find_previous_before(
                pool,
                &instrument.currency,
                BASE_CURRENCY,
                FX_PROVIDER,
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
        price_mapping_enabled,
        latest_price,
        previous_price,
        latest_fx,
        previous_fx,
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
    }
}

pub(super) fn price_snapshot_response(
    snapshot: &crate::domain::PriceSnapshot,
) -> PriceSnapshotResponse {
    PriceSnapshotResponse {
        date: snapshot.date.format("%Y-%m-%d").to_string(),
        close: money_string(snapshot.close),
        currency: snapshot.currency.clone(),
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
    let raw = value.round_dp(2).to_string();
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

fn price_candidate(row: prices::PriceRow) -> Option<PriceCandidate> {
    let date = row.date_value().ok()?;
    let close = row.close_decimal().ok()?;
    Some(PriceCandidate {
        date,
        close,
        currency: row.currency,
    })
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

fn serialize_freshness(freshness: DataFreshness) -> String {
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
