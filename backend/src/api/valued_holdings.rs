use std::collections::BTreeMap;

use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::sqlite::SqlitePool;

use crate::api::error::ApiError;
use crate::api::valuation::load_valuation_inputs;
use crate::db::{instruments, transactions};
use crate::domain::{
    derive_position, value_position, Availability, ConvictionLevel, ConvictionTargetInput,
    FxSnapshot, MarketValueState, Position, PriceSnapshot, ValuationReason, ValuedHolding,
};

#[derive(Debug)]
pub(crate) struct ValuedOpenHolding {
    pub(crate) instrument: instruments::InstrumentRow,
    pub(crate) position: Position,
    pub(crate) conviction: ConvictionLevel,
    pub(crate) valuation: Option<ValuedHolding>,
}

impl ValuedOpenHolding {
    pub(crate) fn conviction_target_input(&self) -> ConvictionTargetInput {
        ConvictionTargetInput {
            instrument_id: self.instrument.id,
            conviction: self.conviction,
            market_value: self.market_value_state(),
            open_quantity: self.position.quantity,
            has_positive_price: self.has_positive_price(),
        }
    }

    pub(crate) fn market_value_state(&self) -> MarketValueState {
        match &self.valuation {
            Some(valuation) => match &valuation.market_value_base {
                Availability::Available(value) => MarketValueState::Available(*value),
                Availability::Unavailable { .. } => MarketValueState::Unavailable,
            },
            None => MarketValueState::MappingDisabled,
        }
    }

    pub(crate) fn market_value_reasons(&self) -> Vec<ValuationReason> {
        match &self.valuation {
            Some(valuation) => valuation.market_value_base.reasons(),
            None => Vec::new(),
        }
    }

    pub(crate) fn latest_price_snapshot(&self) -> Option<&PriceSnapshot> {
        self.valuation.as_ref()?.latest_price.as_ref()
    }

    pub(crate) fn latest_fx_snapshot(&self) -> Option<&FxSnapshot> {
        self.valuation.as_ref()?.latest_fx.as_ref()
    }

    pub(crate) fn has_positive_price(&self) -> bool {
        self.latest_price_snapshot()
            .is_some_and(|snapshot| snapshot.close > Decimal::ZERO)
    }
}

/// Loads valued positions for all instruments, including zero-quantity watchlist rows.
pub async fn load_valued_holdings(
    pool: &SqlitePool,
    valuation_date: NaiveDate,
) -> Result<Vec<ValuedOpenHolding>, ApiError> {
    let instruments_list = instruments::list(pool).await?;
    let transaction_rows = transactions::all_for_holdings(pool).await?;
    let mut ledgers = BTreeMap::new();

    for row in &transaction_rows {
        ledgers
            .entry(row.instrument_id)
            .or_insert_with(Vec::new)
            .push(row.to_ledger()?);
    }

    let mut holdings = Vec::new();
    for instrument in &instruments_list {
        let ledger = ledgers.remove(&instrument.id).unwrap_or_default();
        let position = derive_position(&ledger).map_err(|error| {
            ApiError::internal(format!(
                "inconsistent stored ledger for instrument {}: {error:?}",
                instrument.id
            ))
        })?;
        let valuation_inputs = load_valuation_inputs(pool, instrument, valuation_date).await?;
        let valuation = if valuation_inputs.price_mapping_enabled {
            Some(value_position(
                &position,
                &instrument.currency,
                valuation_date,
                valuation_inputs.latest_price,
                valuation_inputs.previous_price,
                valuation_inputs.latest_fx,
                valuation_inputs.previous_fx,
            ))
        } else {
            None
        };

        let conviction = ConvictionLevel::from_db_str(&instrument.conviction).ok_or_else(|| {
            ApiError::internal(format!(
                "stored unknown conviction {:?} for instrument {}",
                instrument.conviction, instrument.id
            ))
        })?;

        holdings.push(ValuedOpenHolding {
            instrument: instrument.clone(),
            position,
            conviction,
            valuation,
        });
    }

    Ok(holdings)
}
