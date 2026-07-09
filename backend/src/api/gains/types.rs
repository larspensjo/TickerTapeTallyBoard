use serde::{Deserialize, Serialize};

use crate::api::instruments::InstrumentResponse;
use crate::api::valuation::{AvailabilityResponse, FxSnapshotResponse, PriceSnapshotResponse};

#[derive(Debug, Deserialize)]
pub struct GainsQuery {
    #[serde(default)]
    pub(super) include_closed: bool,
    pub(super) start_date: Option<String>,
    pub(super) end_date: Option<String>,
    pub(super) method: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ReportPeriodResponse {
    pub start_date: Option<String>,
    pub end_date: String,
}

#[derive(Debug, Serialize)]
pub struct GainsResponse {
    pub as_of_date: String,
    pub base_currency: String,
    pub include_closed_positions: bool,
    pub report_period: ReportPeriodResponse,
    pub percentage_method: String,
    pub display_percent_kind: String,
    pub summary: SummaryResponse,
    pub totals: TotalsResponse,
    pub rows: Vec<GainRow>,
}

#[derive(Debug, Serialize)]
pub struct SummaryResponse {
    pub market_value_base: AvailabilityResponse,
    pub cost_basis_base: AvailabilityResponse,
    pub price_effect_base: AvailabilityResponse,
    pub fx_effect_base: AvailabilityResponse,
    pub unrealized_gain_base: AvailabilityResponse,
    pub unrealized_gain_percent: AvailabilityResponse,
    pub day_change_base: AvailabilityResponse,
    pub day_change_percent: AvailabilityResponse,
    pub excluded_rows: usize,
}

#[derive(Debug, Serialize)]
pub struct TotalsResponse {
    pub capital_gain_base: AvailabilityResponse,
    pub capital_gain_percent: AvailabilityResponse,
    pub income_base: AvailabilityResponse,
    pub income_percent: AvailabilityResponse,
    pub currency_gain_base: AvailabilityResponse,
    pub currency_gain_percent: AvailabilityResponse,
    pub total_return_base: AvailabilityResponse,
    pub total_return_percent: AvailabilityResponse,
    pub excluded_rows: usize,
}

#[derive(Debug, Serialize)]
pub struct GainRow {
    pub instrument: InstrumentResponse,
    pub quantity: i64,
    pub cost_basis_native: String,
    pub cost_basis_base: AvailabilityResponse,
    pub performance_start_date: Option<String>,
    pub performance_denominator_base: AvailabilityResponse,
    pub capital_gain_base: AvailabilityResponse,
    pub capital_gain_percent: AvailabilityResponse,
    pub currency_gain_base: AvailabilityResponse,
    pub currency_gain_percent: AvailabilityResponse,
    pub income_base: AvailabilityResponse,
    pub total_return_base: AvailabilityResponse,
    pub total_return_percent: AvailabilityResponse,
    pub price_effect_base: AvailabilityResponse,
    pub fx_effect_base: AvailabilityResponse,
    pub latest_price: Option<PriceSnapshotResponse>,
    pub previous_price: Option<PriceSnapshotResponse>,
    pub latest_fx: Option<FxSnapshotResponse>,
    pub previous_fx: Option<FxSnapshotResponse>,
    pub market_value_native: AvailabilityResponse,
    pub market_value_base: AvailabilityResponse,
    pub proceeds_native: AvailabilityResponse,
    pub proceeds_base: AvailabilityResponse,
    pub held_fee_component_base: AvailabilityResponse,
    pub realized_fee_base: AvailabilityResponse,
    pub realized_sell_brokerage_base: AvailabilityResponse,
    pub brokerage_total_base: AvailabilityResponse,
    pub unrealized_price_effect_base: AvailabilityResponse,
    pub unrealized_fx_effect_base: AvailabilityResponse,
    pub unrealized_gain_base: AvailabilityResponse,
    pub unrealized_gain_percent: AvailabilityResponse,
    pub realized_gain_base: AvailabilityResponse,
    pub realized_cost_basis_base: AvailabilityResponse,
    pub day_change_base: AvailabilityResponse,
    pub day_change_percent: AvailabilityResponse,
    pub reasons: Vec<String>,
    pub position_status: GainPositionStatus,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GainPositionStatus {
    Open,
    Closed,
}
