use rust_decimal::Decimal;

use crate::api::valuation::{money_string, AvailabilityResponse};
use crate::domain::{
    Availability, BaseAmount, RealizedGain, UnavailableReason, ValuationReason, ValuedHolding,
};

use super::rows::{dedup_valuation_reasons, serialize_reasons};
use super::types::PortfolioWaterfallResponse;

#[derive(Clone, Debug)]
pub(super) enum IncomeInput {
    Available(Decimal),
    NotTracked,
    Unavailable(Vec<ValuationReason>),
}

trait BaseAmountReasonExt {
    fn reasons(&self) -> Vec<ValuationReason>;
}

impl BaseAmountReasonExt for BaseAmount {
    fn reasons(&self) -> Vec<ValuationReason> {
        match self {
            BaseAmount::Available(_) => Vec::new(),
            BaseAmount::Unavailable { reasons } => reasons
                .iter()
                .map(|reason| match reason {
                    UnavailableReason::MissingFx { .. } => ValuationReason::MissingFx,
                })
                .collect(),
        }
    }
}

#[derive(Default)]
pub(super) struct PortfolioWaterfallAccumulator {
    cost_basis_base: Decimal,
    held_fee_component_base: Decimal,
    price_effect_base: Decimal,
    fx_effect_base: Decimal,
    market_value_base: Decimal,
    realized_gain_base: Decimal,
    realized_fee_base: Decimal,
    realized_cost_basis_base: Decimal,
    brokerage_total_base: Decimal,
    income_base: Decimal,
    unrealized_gain_base: Decimal,
    total_return_base: Decimal,
    has_data: bool,
    excluded_rows: usize,
    unavailable_reasons: Vec<ValuationReason>,
    income_available_rows: usize,
    income_not_tracked_rows: usize,
}

impl PortfolioWaterfallAccumulator {
    pub(super) fn add_open(
        &mut self,
        valued_holding: &ValuedHolding,
        realized: &RealizedGain,
        brokerage_total_base: Decimal,
        income: IncomeInput,
    ) {
        let open_exclusion_reasons = open_exclusion_reasons(valued_holding, realized);
        let Some(cost_basis_base) = self.require_value(
            decimal_availability(&valued_holding.cost_basis_base),
            &open_exclusion_reasons,
        ) else {
            return;
        };
        let Some(held_fee_component_base) = self.require_value(
            decimal_availability(&valued_holding.fee_component_base),
            &open_exclusion_reasons,
        ) else {
            return;
        };
        let Some(price_effect_base) = self.require_value(
            decimal_availability(&valued_holding.price_effect_base),
            &open_exclusion_reasons,
        ) else {
            return;
        };
        let Some(fx_effect_base) = self.require_value(
            decimal_availability(&valued_holding.fx_effect_base),
            &open_exclusion_reasons,
        ) else {
            return;
        };
        let Some(market_value_base) = self.require_value(
            decimal_availability(&valued_holding.market_value_base),
            &open_exclusion_reasons,
        ) else {
            return;
        };
        let Some(unrealized_gain_base) = self.require_value(
            decimal_availability(&valued_holding.unrealized_gain_base),
            &open_exclusion_reasons,
        ) else {
            return;
        };
        let Some(realized_gain_base) = self.require_value(
            base_amount_decimal(&realized.gain_base),
            &open_exclusion_reasons,
        ) else {
            return;
        };
        let Some(realized_fee_base) = self.require_value(
            base_amount_decimal(&realized.fee_base),
            &open_exclusion_reasons,
        ) else {
            return;
        };
        let Some(realized_cost_basis_base) = self.require_value(
            base_amount_decimal(&realized.cost_basis_base),
            &open_exclusion_reasons,
        ) else {
            return;
        };
        let (income_base, income_tracked) =
            match income_contribution(&income, &mut self.income_not_tracked_rows) {
                Ok(result) => result,
                Err(reasons) => {
                    self.exclude_row(&reasons);
                    return;
                }
            };

        self.cost_basis_base += cost_basis_base;
        self.held_fee_component_base += held_fee_component_base;
        self.price_effect_base += price_effect_base;
        self.fx_effect_base += fx_effect_base;
        self.market_value_base += market_value_base;
        self.realized_gain_base += realized_gain_base;
        self.realized_fee_base += realized_fee_base;
        self.realized_cost_basis_base += realized_cost_basis_base;
        self.brokerage_total_base += brokerage_total_base;
        self.income_base += income_base;
        self.unrealized_gain_base += unrealized_gain_base;
        self.total_return_base += unrealized_gain_base + realized_gain_base + income_base;
        self.has_data = true;
        if income_tracked {
            self.income_available_rows += 1;
        }
    }

    pub(super) fn add_closed(
        &mut self,
        realized: &RealizedGain,
        brokerage_total_base: Decimal,
        income: IncomeInput,
    ) {
        let Some(realized_gain_base) = base_amount_decimal(&realized.gain_base) else {
            self.exclude_row(&[
                realized.gain_base.reasons(),
                realized.fee_base.reasons(),
                realized.cost_basis_base.reasons(),
            ]);
            return;
        };
        let Some(realized_fee_base) = base_amount_decimal(&realized.fee_base) else {
            self.exclude_row(&[
                realized.gain_base.reasons(),
                realized.fee_base.reasons(),
                realized.cost_basis_base.reasons(),
            ]);
            return;
        };
        let Some(realized_cost_basis_base) = base_amount_decimal(&realized.cost_basis_base) else {
            self.exclude_row(&[
                realized.gain_base.reasons(),
                realized.fee_base.reasons(),
                realized.cost_basis_base.reasons(),
            ]);
            return;
        };
        let (income_base, income_tracked) =
            match income_contribution(&income, &mut self.income_not_tracked_rows) {
                Ok(result) => result,
                Err(reasons) => {
                    self.exclude_row(&reasons);
                    return;
                }
            };

        self.realized_gain_base += realized_gain_base;
        self.realized_fee_base += realized_fee_base;
        self.realized_cost_basis_base += realized_cost_basis_base;
        self.brokerage_total_base += brokerage_total_base;
        self.income_base += income_base;
        self.total_return_base += realized_gain_base + income_base;
        self.has_data = true;
        if income_tracked {
            self.income_available_rows += 1;
        }
    }

    fn exclude_row(&mut self, reasons: &[Vec<ValuationReason>]) {
        self.excluded_rows += 1;
        for source in reasons {
            self.unavailable_reasons.extend_from_slice(source);
        }
        dedup_valuation_reasons(&mut self.unavailable_reasons);
    }

    fn require_value<T>(
        &mut self,
        value: Option<T>,
        exclusion_reasons: &[Vec<ValuationReason>],
    ) -> Option<T> {
        if value.is_none() {
            self.exclude_row(exclusion_reasons);
        }
        value
    }

    pub(super) fn into_response(self) -> PortfolioWaterfallResponse {
        let unavailable = AvailabilityResponse::Unavailable {
            reasons: serialize_reasons(&self.unavailable_reasons),
        };
        let available = |value: Decimal| AvailabilityResponse::Available {
            value: money_string(value),
        };
        if !self.has_data {
            return PortfolioWaterfallResponse {
                cost_basis_base: unavailable.clone(),
                held_fee_component_base: unavailable.clone(),
                price_effect_base: unavailable.clone(),
                fx_effect_base: unavailable.clone(),
                market_value_base: unavailable.clone(),
                realized_gain_base: unavailable.clone(),
                realized_fee_base: unavailable.clone(),
                realized_cost_basis_base: unavailable.clone(),
                brokerage_total_base: unavailable.clone(),
                income_base: unavailable.clone(),
                unrealized_gain_base: unavailable.clone(),
                total_return_base: unavailable,
                income_not_tracked: false,
                excluded_rows: self.excluded_rows,
            };
        }

        let income_not_tracked =
            self.income_available_rows == 0 && self.income_not_tracked_rows > 0;
        PortfolioWaterfallResponse {
            cost_basis_base: available(self.cost_basis_base),
            held_fee_component_base: available(self.held_fee_component_base),
            price_effect_base: available(self.price_effect_base),
            fx_effect_base: available(self.fx_effect_base),
            market_value_base: available(self.market_value_base),
            realized_gain_base: available(self.realized_gain_base),
            realized_fee_base: available(self.realized_fee_base),
            realized_cost_basis_base: available(self.realized_cost_basis_base),
            brokerage_total_base: available(self.brokerage_total_base),
            income_base: available(self.income_base),
            unrealized_gain_base: available(self.unrealized_gain_base),
            total_return_base: available(self.total_return_base),
            income_not_tracked,
            excluded_rows: self.excluded_rows,
        }
    }
}

fn decimal_availability(value: &Availability<Decimal>) -> Option<Decimal> {
    match value {
        Availability::Available(value) => Some(*value),
        Availability::Unavailable { .. } => None,
    }
}

fn base_amount_decimal(value: &BaseAmount) -> Option<Decimal> {
    match value {
        BaseAmount::Available(value) => Some(*value),
        BaseAmount::Unavailable { .. } => None,
    }
}

fn income_contribution(
    income: &IncomeInput,
    income_not_tracked_rows: &mut usize,
) -> Result<(Decimal, bool), Vec<Vec<ValuationReason>>> {
    match income {
        IncomeInput::Available(value) => Ok((*value, true)),
        IncomeInput::NotTracked => {
            *income_not_tracked_rows += 1;
            Ok((Decimal::ZERO, false))
        }
        IncomeInput::Unavailable(reasons) => Err(vec![reasons.clone()]),
    }
}

fn open_exclusion_reasons(
    valued_holding: &ValuedHolding,
    realized: &RealizedGain,
) -> Vec<Vec<ValuationReason>> {
    vec![
        valued_holding.cost_basis_base.reasons(),
        valued_holding.fee_component_base.reasons(),
        valued_holding.price_effect_base.reasons(),
        valued_holding.fx_effect_base.reasons(),
        valued_holding.market_value_base.reasons(),
        valued_holding.unrealized_gain_base.reasons(),
        realized.gain_base.reasons(),
        realized.fee_base.reasons(),
        realized.cost_basis_base.reasons(),
    ]
}

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    use crate::api::valuation::AvailabilityResponse;
    use crate::domain::{Availability, BaseAmount, RealizedGain, ValuedHolding};

    use super::{IncomeInput, PortfolioWaterfallAccumulator};

    fn available(value: &str) -> Availability<Decimal> {
        Availability::Available(value.parse().expect("decimal"))
    }

    fn base_available(value: &str) -> BaseAmount {
        BaseAmount::Available(value.parse().expect("decimal"))
    }

    fn holding(
        cost_basis: &str,
        price_effect: &str,
        fx_effect: &str,
        market_value: &str,
        unrealized_gain: &str,
    ) -> ValuedHolding {
        ValuedHolding {
            quantity: 10,
            cost_basis_native: dec!(0),
            cost_basis_base: available(cost_basis),
            fee_component_base: available("0.00"),
            price_effect_base: available(price_effect),
            fx_effect_base: available(fx_effect),
            latest_price: Availability::Unavailable { reasons: vec![] },
            previous_price: Availability::Unavailable { reasons: vec![] },
            latest_fx: Availability::Unavailable { reasons: vec![] },
            previous_fx: Availability::Unavailable { reasons: vec![] },
            market_value_native: available("0.00"),
            market_value_base: available(market_value),
            unrealized_gain_base: available(unrealized_gain),
            unrealized_gain_percent: available("0.00"),
            day_change_base: available("0.00"),
            day_change_percent: available("0.00"),
            reasons: vec![],
        }
    }

    fn realized(gain: &str, cost_basis: &str, fee: &str) -> RealizedGain {
        RealizedGain {
            sold_quantity: 10,
            proceeds_native: dec!(0),
            cost_basis_native: dec!(0),
            proceeds_base: base_available("0.00"),
            cost_basis_base: base_available(cost_basis),
            price_effect_base: base_available("0.00"),
            fx_effect_base: base_available("0.00"),
            gain_base: base_available(gain),
            fee_base: base_available(fee),
            sell_brokerage_base: dec!(0),
        }
    }

    #[test]
    fn income_not_tracked_sets_flag_without_excluding_row() {
        let mut accum = PortfolioWaterfallAccumulator::default();
        accum.add_open(
            &holding("100.00", "10.00", "0.00", "110.00", "10.00"),
            &realized("5.00", "20.00", "1.00"),
            dec!(2),
            IncomeInput::NotTracked,
        );
        let response = accum.into_response();
        assert!(response.income_not_tracked);
        match response.income_base {
            AvailabilityResponse::Available { value } => assert_eq!(value, "0.00"),
            other => panic!("unexpected income base: {other:?}"),
        }
        assert_eq!(response.excluded_rows, 0);
    }
}
