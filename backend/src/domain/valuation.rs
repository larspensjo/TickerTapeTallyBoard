use super::performance::split_factor;
use super::{
    derive_position, BaseCostBasis, LedgerError, LedgerTransaction, Position, TransactionKind,
    UnavailableReason,
};
use chrono::{Datelike, NaiveDate, Weekday};
use rust_decimal::Decimal;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataFreshness {
    Fresh,
    MinorStale { trading_days: i64 },
    WarningStale { trading_days: i64 },
}

impl DataFreshness {
    fn from_age(trading_days: i64) -> Self {
        match trading_days {
            0 => Self::Fresh,
            1..=2 => Self::MinorStale { trading_days },
            _ => Self::WarningStale { trading_days },
        }
    }

    pub fn trading_days(self) -> i64 {
        match self {
            Self::Fresh => 0,
            Self::MinorStale { trading_days } | Self::WarningStale { trading_days } => trading_days,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValuationReason {
    MissingPrice,
    MissingFx,
    MissingPreviousClose,
    MissingPreviousFx,
    StalePrice { trading_days: i64 },
    StaleFx { trading_days: i64 },
    ZeroCostBasis,
    ZeroPreviousMarketValue,
    BaseCostBasisUnavailable { reasons: Vec<UnavailableReason> },
    // performance-specific variants:
    MissingStartPrice,
    MissingEndPrice,
    MissingStartFx,
    MissingEndFx,
    MissingTransactionPrice { transaction_id: i64 },
    MissingTransactionFx { transaction_id: i64 },
    ZeroOrInvalidPerformanceDenominator,
    PerformanceDidNotConverge,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PriceCandidate {
    pub date: NaiveDate,
    pub close: Decimal,
    pub currency: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FxCandidate {
    pub date: NaiveDate,
    pub rate: Decimal,
    pub base: String,
    pub quote: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PriceSnapshot {
    pub date: NaiveDate,
    pub close: Decimal,
    pub currency: String,
    pub freshness: DataFreshness,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FxSnapshot {
    pub date: NaiveDate,
    pub rate: Decimal,
    pub base: String,
    pub quote: String,
    pub freshness: DataFreshness,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FxApplied {
    pub rate: Decimal,
    pub date: NaiveDate,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PricePoint {
    pub date: NaiveDate,
    pub close: Decimal,
    pub close_base: Availability<Decimal>,
    pub fx: Option<FxApplied>,
}

/// Build a per-instrument daily series converted to SEK.
///
/// `prices` and `fx_rates` must both be sorted by date ascending. A single
/// forward pass advances an FX index while `fx.date <= point.date`, retaining
/// the last seen rate as the carry-forward value. Rows whose currency differs
/// from `native_currency` are dropped (internal data error). SEK instruments
/// take the identity path with `fx` omitted.
pub fn build_price_history(
    native_currency: &str,
    prices: &[PriceCandidate],
    fx_rates: &[FxCandidate],
) -> Vec<PricePoint> {
    let is_base = native_currency.eq_ignore_ascii_case("SEK");
    let mut points = Vec::new();
    let mut fx_idx = 0usize;
    let mut current_fx: Option<&FxCandidate> = None;

    for price in prices {
        if !price.currency.eq_ignore_ascii_case(native_currency) {
            continue;
        }

        if is_base {
            points.push(PricePoint {
                date: price.date,
                close: price.close,
                close_base: Availability::available(price.close),
                fx: None,
            });
            continue;
        }

        while fx_idx < fx_rates.len() && fx_rates[fx_idx].date <= price.date {
            current_fx = Some(&fx_rates[fx_idx]);
            fx_idx += 1;
        }

        match current_fx {
            Some(fx) => points.push(PricePoint {
                date: price.date,
                close: price.close,
                close_base: Availability::available(price.close * fx.rate),
                fx: Some(FxApplied {
                    rate: fx.rate,
                    date: fx.date,
                }),
            }),
            None => points.push(PricePoint {
                date: price.date,
                close: price.close,
                close_base: Availability::unavailable(ValuationReason::MissingFx),
                fx: None,
            }),
        }
    }

    points
}

#[derive(Clone, Debug)]
pub struct ValueHistoryInstrument {
    pub native_currency: String,
    pub ledger: Vec<LedgerTransaction>,
    pub prices: Vec<PriceCandidate>,
    pub fx_rates: Vec<FxCandidate>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ValueHistoryPoint {
    pub date: NaiveDate,
    pub value_base: Decimal,
    pub invested_base: Option<Decimal>,
    pub incomplete: bool,
    pub included_count: usize,
    pub excluded_count: usize,
}

struct InvestedCashFlow {
    date: NaiveDate,
    delta: Decimal,
}

/// Collect SEK cash-flow deltas from Buy/Sell trades and the earliest date from
/// which invested capital becomes unavailable because a non-SEK trade lacks FX.
/// Buys add cash out (price·qty·fx + brokerage); sells subtract cash returned
/// (price·|qty|·fx − brokerage). Splits and dividends are ignored.
fn invested_cash_flows(
    instruments: &[ValueHistoryInstrument],
) -> Result<(Vec<InvestedCashFlow>, Option<NaiveDate>), LedgerError> {
    let mut events = Vec::new();
    let mut unavailable_from: Option<NaiveDate> = None;
    let mark_unavailable = |date: NaiveDate, slot: &mut Option<NaiveDate>| {
        *slot = Some(match *slot {
            Some(current) => current.min(date),
            None => date,
        });
    };

    for inst in instruments {
        let is_base = inst.native_currency.eq_ignore_ascii_case("SEK");
        for tx in &inst.ledger {
            let (price, signed_qty) = match tx.kind {
                TransactionKind::Buy => (
                    tx.price.ok_or(LedgerError::BuyMissingPrice {
                        transaction_id: tx.id,
                    })?,
                    Decimal::from(tx.quantity),
                ),
                TransactionKind::Sell => (
                    tx.price.ok_or(LedgerError::SellMissingPrice {
                        transaction_id: tx.id,
                    })?,
                    // Sell quantity is negative; cash returned reduces invested.
                    Decimal::from(tx.quantity),
                ),
                TransactionKind::Split | TransactionKind::Dividend => continue,
            };

            let fx = if is_base {
                Some(Decimal::ONE)
            } else {
                tx.fx_rate_to_base
            };
            let Some(fx) = fx else {
                mark_unavailable(tx.trade_date, &mut unavailable_from);
                continue;
            };

            // Both buys and sells *add* brokerage_base. Buy: +(price·qty·fx) +
            // brokerage raises net invested. Sell: signed_qty is negative, so
            // price·signed_qty·fx is already the (negative) cash returned;
            // adding brokerage reduces that cash returned, i.e. keeps net
            // invested higher. Matches the authoritative formula above:
            //   −(price·|qty|·fx − brokerage) = price·signed_qty·fx + brokerage.
            let delta = price * signed_qty * fx + tx.brokerage_base;
            events.push(InvestedCashFlow {
                date: tx.trade_date,
                delta,
            });
        }
    }

    events.sort_by_key(|event| event.date);
    Ok((events, unavailable_from))
}

/// Build the portfolio value series in SEK from a union of price and FX dates.
///
/// The date spine starts at the first Buy and includes every supplied price or
/// FX date in the optional window. For each date, open positions are derived
/// from ledger rows up to that date, future splits are applied so historic
/// quantities line up with split-adjusted price history, and the latest price
/// and FX rows on or before the date are carried forward. Instruments missing
/// either input are counted as excluded; dates where every open position is
/// excluded are omitted so the chart never shows a spurious zero portfolio
/// value. Non-SEK FX rows must be for `native_currency -> SEK`; mismatched rows
/// are ignored to keep this pure helper safe even if callers forget to prefilter.
pub fn build_value_history(
    instruments: &[ValueHistoryInstrument],
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
) -> Result<Vec<ValueHistoryPoint>, LedgerError> {
    let first_buy = instruments
        .iter()
        .flat_map(|inst| inst.ledger.iter())
        .filter(|tx| tx.kind == TransactionKind::Buy)
        .map(|tx| tx.trade_date)
        .min();
    let Some(first_buy) = first_buy else {
        return Ok(Vec::new());
    };

    let (cash_flows, invested_unavailable_from) = invested_cash_flows(instruments)?;
    let mut next_flow = 0usize;
    let mut invested_total = Decimal::ZERO;

    let mut spine: BTreeSet<NaiveDate> = BTreeSet::new();
    for inst in instruments {
        for price in &inst.prices {
            spine.insert(price.date);
        }
        for fx in &inst.fx_rates {
            spine.insert(fx.date);
        }
    }

    let mut points = Vec::new();
    for date in spine {
        if date < first_buy {
            continue;
        }
        if from.is_some_and(|from| date < from) || to.is_some_and(|to| date > to) {
            continue;
        }

        let mut value_base = Decimal::ZERO;
        let mut included_count = 0usize;
        let mut excluded_count = 0usize;

        for inst in instruments {
            let active: Vec<LedgerTransaction> = inst
                .ledger
                .iter()
                .filter(|tx| tx.trade_date <= date)
                .cloned()
                .collect();
            let position = derive_position(&active)?;
            if position.quantity == 0 {
                continue;
            }

            let future: Vec<LedgerTransaction> = inst
                .ledger
                .iter()
                .filter(|tx| tx.trade_date > date)
                .cloned()
                .collect();
            let factor = split_factor(&future, position.quantity)?;
            let adjusted_qty = Decimal::from(position.quantity) * factor;

            let close = inst
                .prices
                .iter()
                .rfind(|price| {
                    price.date <= date && price.currency.eq_ignore_ascii_case(&inst.native_currency)
                })
                .map(|price| price.close);

            let rate = if inst.native_currency.eq_ignore_ascii_case("SEK") {
                Some(Decimal::ONE)
            } else {
                inst.fx_rates
                    .iter()
                    .rfind(|fx| {
                        fx.date <= date
                            && fx.base.eq_ignore_ascii_case(&inst.native_currency)
                            && fx.quote.eq_ignore_ascii_case("SEK")
                    })
                    .map(|fx| fx.rate)
            };

            match (close, rate) {
                (Some(close), Some(rate)) => {
                    value_base += adjusted_qty * close * rate;
                    included_count += 1;
                }
                _ => excluded_count += 1,
            }
        }

        if included_count == 0 {
            continue;
        }

        while next_flow < cash_flows.len() && cash_flows[next_flow].date <= date {
            invested_total += cash_flows[next_flow].delta;
            next_flow += 1;
        }
        let invested_base = match invested_unavailable_from {
            Some(unavailable) if date >= unavailable => None,
            _ => Some(invested_total),
        };

        points.push(ValueHistoryPoint {
            date,
            value_base,
            invested_base,
            incomplete: excluded_count > 0,
            included_count,
            excluded_count,
        });
    }

    Ok(points)
}

#[derive(Clone, Debug, PartialEq)]
pub enum Availability<T> {
    Available(T),
    Unavailable { reasons: Vec<ValuationReason> },
}

impl<T> Availability<T> {
    pub fn available(value: T) -> Self {
        Self::Available(value)
    }

    pub fn unavailable(reason: ValuationReason) -> Self {
        Self::Unavailable {
            reasons: vec![reason],
        }
    }

    pub fn unavailable_empty() -> Self {
        Self::Unavailable {
            reasons: Vec::new(),
        }
    }

    pub fn as_ref(&self) -> Option<&T> {
        match self {
            Self::Available(value) => Some(value),
            Self::Unavailable { .. } => None,
        }
    }

    pub fn reasons(&self) -> Vec<ValuationReason> {
        match self {
            Self::Available(_) => Vec::new(),
            Self::Unavailable { reasons } => reasons.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ValuedHolding {
    pub quantity: i64,
    pub cost_basis_native: Decimal,
    pub cost_basis_base: Availability<Decimal>,
    pub fee_component_base: Availability<Decimal>,
    pub price_effect_base: Availability<Decimal>,
    pub fx_effect_base: Availability<Decimal>,
    pub latest_price: Availability<PriceSnapshot>,
    pub previous_price: Availability<PriceSnapshot>,
    pub latest_fx: Availability<FxSnapshot>,
    pub previous_fx: Availability<FxSnapshot>,
    pub market_value_native: Availability<Decimal>,
    pub market_value_base: Availability<Decimal>,
    pub unrealized_gain_base: Availability<Decimal>,
    pub unrealized_gain_percent: Availability<Decimal>,
    pub day_change_base: Availability<Decimal>,
    pub day_change_percent: Availability<Decimal>,
    pub reasons: Vec<ValuationReason>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ValuationSummary {
    pub market_value_base: Availability<Decimal>,
    pub cost_basis_base: Availability<Decimal>,
    pub price_effect_base: Availability<Decimal>,
    pub fx_effect_base: Availability<Decimal>,
    pub unrealized_gain_base: Availability<Decimal>,
    pub unrealized_gain_percent: Availability<Decimal>,
    pub day_change_base: Availability<Decimal>,
    pub day_change_percent: Availability<Decimal>,
    pub excluded_rows: usize,
}

pub fn value_position(
    position: &Position,
    native_currency: &str,
    valuation_date: NaiveDate,
    latest_price: Option<PriceCandidate>,
    previous_price: Option<PriceCandidate>,
    latest_fx: Option<FxCandidate>,
    previous_fx: Option<FxCandidate>,
) -> ValuedHolding {
    debug_assert!(
        position.quantity >= 0,
        "valuation expects a derived position"
    );

    let latest_price = match latest_price {
        Some(candidate) => {
            let freshness = data_freshness(valuation_date, candidate.date);
            let snapshot = PriceSnapshot {
                date: candidate.date,
                close: candidate.close,
                currency: candidate.currency,
                freshness,
            };
            Availability::available(snapshot)
        }
        None => Availability::unavailable(ValuationReason::MissingPrice),
    };

    let previous_price = match previous_price {
        Some(candidate) => Availability::available(PriceSnapshot {
            date: candidate.date,
            close: candidate.close,
            currency: candidate.currency,
            freshness: data_freshness(valuation_date, candidate.date),
        }),
        None => Availability::unavailable(ValuationReason::MissingPreviousClose),
    };

    let latest_fx = fx_snapshot(native_currency, valuation_date, latest_fx, false);
    let previous_fx = fx_snapshot(native_currency, valuation_date, previous_fx, true);

    let cost_basis_base = match &position.base {
        BaseCostBasis::Available {
            cost_basis_base, ..
        } => Availability::available(*cost_basis_base),
        BaseCostBasis::Unavailable { reasons } => Availability::Unavailable {
            reasons: vec![ValuationReason::BaseCostBasisUnavailable {
                reasons: reasons.clone(),
            }],
        },
    };

    let fee_component_base = match &position.base {
        BaseCostBasis::Available {
            fee_component_base, ..
        } => Some(*fee_component_base),
        BaseCostBasis::Unavailable { .. } => None,
    };

    let market_value_native = match latest_price.as_ref() {
        Some(price) => Availability::available(price.close * Decimal::from(position.quantity)),
        None => Availability::unavailable(ValuationReason::MissingPrice),
    };

    let market_value_base = match (market_value_native.as_ref(), latest_fx.as_ref()) {
        (Some(native_value), Some(fx)) => Availability::available(*native_value * fx.rate),
        _ => Availability::Unavailable {
            reasons: merge_reasons(
                &market_value_native.reasons(),
                &latest_fx.reasons(),
                ValuationReason::MissingPrice,
            ),
        },
    };

    let previous_market_value_base = match (previous_price.as_ref(), previous_fx.as_ref()) {
        (Some(price), Some(fx)) => {
            Availability::available(price.close * fx.rate * Decimal::from(position.quantity))
        }
        _ => {
            let mut missing = Vec::new();
            missing.extend(previous_price.reasons());
            missing.extend(previous_fx.reasons());
            unavailable_or_first(missing, ValuationReason::MissingPreviousClose)
        }
    };

    let unrealized_gain_base = match (market_value_base.as_ref(), cost_basis_base.as_ref()) {
        (Some(market_value), Some(cost_basis)) => {
            Availability::available(*market_value - *cost_basis)
        }
        _ => Availability::Unavailable {
            reasons: merge_reasons(
                &market_value_base.reasons(),
                &cost_basis_base.reasons(),
                ValuationReason::MissingPrice,
            ),
        },
    };

    let unrealized_gain_percent = match (unrealized_gain_base.as_ref(), cost_basis_base.as_ref()) {
        (Some(gain), Some(cost_basis)) if *cost_basis != Decimal::ZERO => {
            Availability::available((*gain / *cost_basis) * Decimal::from(100))
        }
        (Some(_), Some(_)) => Availability::unavailable(ValuationReason::ZeroCostBasis),
        _ => Availability::Unavailable {
            reasons: merge_reasons(
                &unrealized_gain_base.reasons(),
                &cost_basis_base.reasons(),
                ValuationReason::MissingPrice,
            ),
        },
    };

    let effect_reasons = effect_reasons(
        &market_value_native,
        &cost_basis_base,
        &latest_fx,
        position.cost_basis_native,
    );

    let price_effect_base = match (
        market_value_native.as_ref(),
        cost_basis_base.as_ref(),
        latest_fx.as_ref(),
        fee_component_base,
    ) {
        (Some(native_value), Some(_), Some(fx), Some(fee_component_base))
            if position.cost_basis_native != Decimal::ZERO =>
        {
            Availability::available(
                (*native_value - position.cost_basis_native) * fx.rate - fee_component_base,
            )
        }
        _ => Availability::Unavailable {
            reasons: effect_reasons.clone(),
        },
    };

    let fx_effect_base = match (
        market_value_native.as_ref(),
        cost_basis_base.as_ref(),
        latest_fx.as_ref(),
        fee_component_base,
    ) {
        (Some(_native_value), Some(cost_basis_base), Some(fx), Some(fee_component_base))
            if position.cost_basis_native != Decimal::ZERO =>
        {
            let gross_base = *cost_basis_base - fee_component_base;
            Availability::available(position.cost_basis_native * fx.rate - gross_base)
        }
        _ => Availability::Unavailable {
            reasons: effect_reasons,
        },
    };

    let day_change_base = match (
        latest_price.as_ref(),
        previous_price.as_ref(),
        latest_fx.as_ref(),
        previous_fx.as_ref(),
    ) {
        (Some(latest), Some(previous), Some(latest_fx), Some(previous_fx)) => {
            let latest_value = latest.close * latest_fx.rate;
            let previous_value = previous.close * previous_fx.rate;
            Availability::available(
                (latest_value - previous_value) * Decimal::from(position.quantity),
            )
        }
        _ => Availability::Unavailable {
            reasons: merge_many_reasons(
                &[
                    latest_price.reasons(),
                    previous_price.reasons(),
                    latest_fx.reasons(),
                    previous_fx.reasons(),
                ],
                ValuationReason::MissingPreviousClose,
            ),
        },
    };

    let day_change_percent = match (
        day_change_base.as_ref(),
        previous_market_value_base.as_ref(),
    ) {
        (Some(day_change), Some(previous_market_value))
            if *previous_market_value != Decimal::ZERO =>
        {
            Availability::available((*day_change / *previous_market_value) * Decimal::from(100))
        }
        (Some(_), Some(_)) => Availability::unavailable(ValuationReason::ZeroPreviousMarketValue),
        _ => Availability::Unavailable {
            reasons: merge_reasons(
                &day_change_base.reasons(),
                &previous_market_value_base.reasons(),
                ValuationReason::MissingPreviousClose,
            ),
        },
    };

    let mut reasons = Vec::new();
    append_snapshot_reasons(&mut reasons, &latest_price);
    append_fx_reasons(&mut reasons, &latest_fx);
    append_amount_reasons(&mut reasons, &market_value_native);
    append_amount_reasons(&mut reasons, &market_value_base);
    append_amount_reasons(&mut reasons, &previous_market_value_base);
    append_amount_reasons(&mut reasons, &price_effect_base);
    append_amount_reasons(&mut reasons, &fx_effect_base);
    append_amount_reasons(&mut reasons, &unrealized_gain_base);
    append_amount_reasons(&mut reasons, &unrealized_gain_percent);
    append_amount_reasons(&mut reasons, &day_change_base);
    append_amount_reasons(&mut reasons, &day_change_percent);
    append_amount_reasons(&mut reasons, &cost_basis_base);
    dedup_reasons(&mut reasons);

    ValuedHolding {
        quantity: position.quantity,
        cost_basis_native: position.cost_basis_native,
        cost_basis_base,
        fee_component_base: match &position.base {
            BaseCostBasis::Available {
                fee_component_base, ..
            } => Availability::available(*fee_component_base),
            BaseCostBasis::Unavailable { reasons } => Availability::Unavailable {
                reasons: vec![ValuationReason::BaseCostBasisUnavailable {
                    reasons: reasons.clone(),
                }],
            },
        },
        price_effect_base,
        fx_effect_base,
        latest_price,
        previous_price,
        latest_fx,
        previous_fx,
        market_value_native,
        market_value_base,
        unrealized_gain_base,
        unrealized_gain_percent,
        day_change_base,
        day_change_percent,
        reasons,
    }
}

pub fn summarize_holdings(rows: &[ValuedHolding]) -> ValuationSummary {
    let mut market_value_base = Decimal::ZERO;
    let mut cost_basis_base = Decimal::ZERO;
    let mut price_effect_base = Decimal::ZERO;
    let mut fx_effect_base = Decimal::ZERO;
    let mut day_change_base = Decimal::ZERO;
    let mut previous_market_value_base = Decimal::ZERO;
    let mut included_rows = 0usize;
    let mut effect_rows = 0usize;
    let mut day_change_rows = 0usize;
    let mut excluded_rows = 0usize;

    for row in rows {
        match (row.market_value_base.as_ref(), row.cost_basis_base.as_ref()) {
            (Some(market_value), Some(cost_basis)) => {
                included_rows += 1;
                market_value_base += *market_value;
                cost_basis_base += *cost_basis;
                if let (Some(price_effect), Some(fx_effect)) =
                    (row.price_effect_base.as_ref(), row.fx_effect_base.as_ref())
                {
                    price_effect_base += *price_effect;
                    fx_effect_base += *fx_effect;
                    effect_rows += 1;
                }
                if let Some(day_change) = row.day_change_base.as_ref() {
                    day_change_base += *day_change;
                    previous_market_value_base += *market_value - *day_change;
                    day_change_rows += 1;
                }
            }
            _ => excluded_rows += 1,
        }
    }

    let market_value_base = if included_rows > 0 {
        Availability::available(market_value_base)
    } else {
        Availability::unavailable_empty()
    };
    let cost_basis_base = if included_rows > 0 {
        Availability::available(cost_basis_base)
    } else {
        Availability::unavailable_empty()
    };
    let price_effect_base = if effect_rows > 0 {
        Availability::available(price_effect_base)
    } else {
        Availability::unavailable_empty()
    };
    let fx_effect_base = if effect_rows > 0 {
        Availability::available(fx_effect_base)
    } else {
        Availability::unavailable_empty()
    };
    let unrealized_gain_base = match (market_value_base.as_ref(), cost_basis_base.as_ref()) {
        (Some(market_value), Some(cost_basis)) => {
            Availability::available(*market_value - *cost_basis)
        }
        _ => Availability::unavailable_empty(),
    };
    let unrealized_gain_percent = match (unrealized_gain_base.as_ref(), cost_basis_base.as_ref()) {
        (Some(gain), Some(cost_basis)) if *cost_basis != Decimal::ZERO => {
            Availability::available((*gain / *cost_basis) * Decimal::from(100))
        }
        (Some(_), Some(_)) => Availability::unavailable(ValuationReason::ZeroCostBasis),
        _ => Availability::unavailable_empty(),
    };
    let day_change_base = if day_change_rows > 0 {
        Availability::available(day_change_base)
    } else {
        Availability::unavailable_empty()
    };
    let day_change_percent = match day_change_base.as_ref() {
        Some(day_change) if previous_market_value_base != Decimal::ZERO => {
            Availability::available((*day_change / previous_market_value_base) * Decimal::from(100))
        }
        Some(_) => Availability::unavailable(ValuationReason::ZeroPreviousMarketValue),
        None => Availability::unavailable_empty(),
    };

    ValuationSummary {
        market_value_base,
        cost_basis_base,
        price_effect_base,
        fx_effect_base,
        unrealized_gain_base,
        unrealized_gain_percent,
        day_change_base,
        day_change_percent,
        excluded_rows,
    }
}

fn fx_snapshot(
    native_currency: &str,
    valuation_date: NaiveDate,
    candidate: Option<FxCandidate>,
    previous: bool,
) -> Availability<FxSnapshot> {
    if native_currency.eq_ignore_ascii_case("SEK") {
        return Availability::available(FxSnapshot {
            date: valuation_date,
            rate: Decimal::ONE,
            base: "SEK".to_owned(),
            quote: "SEK".to_owned(),
            freshness: DataFreshness::Fresh,
        });
    }

    match candidate {
        Some(candidate) => Availability::available(FxSnapshot {
            date: candidate.date,
            rate: candidate.rate,
            base: candidate.base,
            quote: candidate.quote,
            freshness: data_freshness(valuation_date, candidate.date),
        }),
        None => {
            if previous {
                Availability::unavailable(ValuationReason::MissingPreviousFx)
            } else {
                Availability::unavailable(ValuationReason::MissingFx)
            }
        }
    }
}

fn data_freshness(valuation_date: NaiveDate, data_date: NaiveDate) -> DataFreshness {
    DataFreshness::from_age(trading_days_stale(valuation_date, data_date))
}

fn trading_days_stale(valuation_date: NaiveDate, data_date: NaiveDate) -> i64 {
    if data_date >= valuation_date {
        return 0;
    }

    let mut count = 0;
    let mut day = data_date;
    while let Some(next) = day.succ_opt() {
        if next > valuation_date {
            break;
        }
        if is_weekday(next) {
            count += 1;
        }
        day = next;
    }
    count
}

fn is_weekday(date: NaiveDate) -> bool {
    !matches!(date.weekday(), Weekday::Sat | Weekday::Sun)
}

fn merge_reasons(
    first: &[ValuationReason],
    second: &[ValuationReason],
    fallback: ValuationReason,
) -> Vec<ValuationReason> {
    let mut reasons = Vec::new();
    reasons.extend_from_slice(first);
    reasons.extend_from_slice(second);
    dedup_reasons(&mut reasons);
    if reasons.is_empty() {
        reasons.push(fallback);
    }
    reasons
}

fn merge_many_reasons(
    sources: &[Vec<ValuationReason>],
    fallback: ValuationReason,
) -> Vec<ValuationReason> {
    let mut reasons = Vec::new();
    for source in sources {
        reasons.extend_from_slice(source);
    }
    dedup_reasons(&mut reasons);
    if reasons.is_empty() {
        reasons.push(fallback);
    }
    reasons
}

fn unavailable_or_first(
    reasons: Vec<ValuationReason>,
    fallback: ValuationReason,
) -> Availability<Decimal> {
    if reasons.is_empty() {
        Availability::unavailable(fallback)
    } else {
        Availability::Unavailable { reasons }
    }
}

fn effect_reasons(
    market_value_native: &Availability<Decimal>,
    cost_basis_base: &Availability<Decimal>,
    latest_fx: &Availability<FxSnapshot>,
    cost_basis_native: Decimal,
) -> Vec<ValuationReason> {
    let mut reasons = Vec::new();
    reasons.extend(market_value_native.reasons());
    reasons.extend(cost_basis_base.reasons());
    reasons.extend(latest_fx.reasons());
    if cost_basis_native == Decimal::ZERO {
        reasons.push(ValuationReason::ZeroCostBasis);
    }
    dedup_reasons(&mut reasons);
    reasons
}

fn append_snapshot_reasons(
    reasons: &mut Vec<ValuationReason>,
    state: &Availability<PriceSnapshot>,
) {
    match state {
        Availability::Available(snapshot) => match snapshot.freshness {
            DataFreshness::Fresh => {}
            DataFreshness::MinorStale { trading_days }
            | DataFreshness::WarningStale { trading_days } => {
                reasons.push(ValuationReason::StalePrice { trading_days });
            }
        },
        Availability::Unavailable {
            reasons: state_reasons,
        } => reasons.extend(state_reasons.clone()),
    }
}

fn append_fx_reasons(reasons: &mut Vec<ValuationReason>, state: &Availability<FxSnapshot>) {
    match state {
        Availability::Available(snapshot) => match snapshot.freshness {
            DataFreshness::Fresh => {}
            DataFreshness::MinorStale { trading_days }
            | DataFreshness::WarningStale { trading_days } => {
                reasons.push(ValuationReason::StaleFx { trading_days });
            }
        },
        Availability::Unavailable {
            reasons: state_reasons,
        } => reasons.extend(state_reasons.clone()),
    }
}

fn append_amount_reasons(reasons: &mut Vec<ValuationReason>, state: &Availability<Decimal>) {
    if let Availability::Unavailable {
        reasons: state_reasons,
    } = state
    {
        reasons.extend(state_reasons.clone());
    }
}

fn dedup_reasons(reasons: &mut Vec<ValuationReason>) {
    let mut deduped = Vec::with_capacity(reasons.len());
    for reason in reasons.drain(..) {
        if !deduped.contains(&reason) {
            deduped.push(reason);
        }
    }
    *reasons = deduped;
}

#[cfg(test)]
mod tests {
    use super::{
        build_price_history, build_value_history, summarize_holdings, value_position, Availability,
        DataFreshness, FxApplied, FxCandidate, PriceCandidate, ValuationReason,
        ValueHistoryInstrument,
    };
    use crate::domain::{
        derive_position, BaseCostBasis, LedgerTransaction, Position, TransactionKind,
        UnavailableReason,
    };
    use chrono::NaiveDate;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn d(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("valid date")
    }

    fn buy(
        id: i64,
        date: NaiveDate,
        qty: i64,
        price: Decimal,
        fx: Option<Decimal>,
        _currency: &str,
    ) -> LedgerTransaction {
        LedgerTransaction {
            id,
            trade_date: date,
            kind: TransactionKind::Buy,
            quantity: qty,
            price: Some(price),
            dividend_per_share: None,
            fx_rate_to_base: fx,
            brokerage_base: Decimal::ZERO,
        }
    }

    fn split(id: i64, date: NaiveDate, delta: i64) -> LedgerTransaction {
        LedgerTransaction {
            id,
            trade_date: date,
            kind: TransactionKind::Split,
            quantity: delta,
            price: None,
            dividend_per_share: None,
            fx_rate_to_base: None,
            brokerage_base: Decimal::ZERO,
        }
    }

    fn position(transactions: &[LedgerTransaction]) -> Position {
        derive_position(transactions).expect("derives")
    }

    fn price(date: NaiveDate, close: Decimal, currency: &str) -> PriceCandidate {
        PriceCandidate {
            date,
            close,
            currency: currency.to_owned(),
        }
    }

    fn fx(date: NaiveDate, rate: Decimal, base: &str, quote: &str) -> FxCandidate {
        FxCandidate {
            date,
            rate,
            base: base.to_owned(),
            quote: quote.to_owned(),
        }
    }

    fn vh_instrument(
        currency: &str,
        ledger: Vec<LedgerTransaction>,
        prices: Vec<PriceCandidate>,
        fx_rates: Vec<FxCandidate>,
    ) -> ValueHistoryInstrument {
        ValueHistoryInstrument {
            native_currency: currency.to_owned(),
            ledger,
            prices,
            fx_rates,
        }
    }

    fn ledger_buy(id: i64, date: NaiveDate, qty: i64, price: Decimal) -> LedgerTransaction {
        LedgerTransaction {
            id,
            trade_date: date,
            kind: TransactionKind::Buy,
            quantity: qty,
            price: Some(price),
            dividend_per_share: None,
            fx_rate_to_base: None,
            brokerage_base: Decimal::ZERO,
        }
    }

    fn ledger_sell(id: i64, date: NaiveDate, qty: i64, price: Decimal) -> LedgerTransaction {
        LedgerTransaction {
            id,
            trade_date: date,
            kind: TransactionKind::Sell,
            quantity: -qty,
            price: Some(price),
            dividend_per_share: None,
            fx_rate_to_base: None,
            brokerage_base: Decimal::ZERO,
        }
    }

    fn ledger_buy_fx(
        id: i64,
        date: NaiveDate,
        qty: i64,
        price: Decimal,
        fx_rate_to_base: Option<Decimal>,
    ) -> LedgerTransaction {
        LedgerTransaction {
            fx_rate_to_base,
            ..ledger_buy(id, date, qty, price)
        }
    }

    #[test]
    fn invested_capital_tracks_buy_cost_including_brokerage() {
        // SEK instrument: buy 10 @ 100 with 9 SEK brokerage on 2026-01-02.
        let mut buy = ledger_buy(1, d(2026, 1, 2), 10, dec!(100));
        buy.brokerage_base = dec!(9);
        let inst = ValueHistoryInstrument {
            native_currency: "SEK".to_string(),
            ledger: vec![buy],
            prices: vec![price(d(2026, 1, 2), dec!(100), "SEK")],
            fx_rates: vec![],
        };
        let points = build_value_history(&[inst], None, None).expect("derivable ledger");
        assert_eq!(points.len(), 1);
        // 10*100 + 9 = 1009
        assert_eq!(points[0].invested_base, Some(dec!(1009)));
    }

    #[test]
    fn invested_capital_drops_by_sell_proceeds_net_of_brokerage() {
        // Buy 10 @ 100 (2026-01-02), sell 4 @ 150 with 5 SEK brokerage (2026-01-05).
        let buy = ledger_buy(1, d(2026, 1, 2), 10, dec!(100));
        let mut sell = ledger_sell(2, d(2026, 1, 5), 4, dec!(150));
        sell.brokerage_base = dec!(5);
        let inst = ValueHistoryInstrument {
            native_currency: "SEK".to_string(),
            ledger: vec![buy, sell],
            prices: vec![
                price(d(2026, 1, 2), dec!(100), "SEK"),
                price(d(2026, 1, 5), dec!(150), "SEK"),
            ],
            fx_rates: vec![],
        };
        let points = build_value_history(&[inst], None, None).expect("derivable ledger");
        // Day 1: invested = 1000. Day 2: 1000 - (4*150 - 5) = 1000 - 595 = 405.
        assert_eq!(points[0].invested_base, Some(dec!(1000)));
        assert_eq!(points[1].invested_base, Some(dec!(405)));
    }

    #[test]
    fn invested_capital_uses_trade_time_fx_for_non_sek() {
        // USD instrument: buy 10 @ 100 USD at fx 10 on 2026-01-02.
        let buy = ledger_buy_fx(1, d(2026, 1, 2), 10, dec!(100), Some(dec!(10)));
        let inst = ValueHistoryInstrument {
            native_currency: "USD".to_string(),
            ledger: vec![buy],
            prices: vec![price(d(2026, 1, 2), dec!(100), "USD")],
            fx_rates: vec![fx(d(2026, 1, 2), dec!(10), "USD", "SEK")],
        };
        let points = build_value_history(&[inst], None, None).expect("derivable ledger");
        // 10*100*10 = 10000, no brokerage.
        assert_eq!(points[0].invested_base, Some(dec!(10000)));
    }

    #[test]
    fn invested_capital_unavailable_when_non_sek_trade_lacks_fx() {
        // USD buy missing fx_rate_to_base => invested unavailable from that date.
        let buy = ledger_buy_fx(1, d(2026, 1, 2), 10, dec!(100), None);
        let inst = ValueHistoryInstrument {
            native_currency: "USD".to_string(),
            ledger: vec![buy],
            prices: vec![price(d(2026, 1, 2), dec!(100), "USD")],
            fx_rates: vec![fx(d(2026, 1, 2), dec!(10), "USD", "SEK")],
        };
        let points = build_value_history(&[inst], None, None).expect("derivable ledger");
        assert_eq!(points[0].invested_base, None);
    }

    #[test]
    fn value_history_sek_single_holding_uses_price_dates() {
        let inst = vh_instrument(
            "SEK",
            vec![buy(1, d(2026, 1, 2), 10, dec!(100), Some(dec!(1)), "SEK")],
            vec![
                price(d(2026, 1, 2), dec!(100), "SEK"),
                price(d(2026, 1, 5), dec!(110), "SEK"),
            ],
            vec![],
        );
        let points = build_value_history(&[inst], None, None).expect("derivable ledger");
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].date, d(2026, 1, 2));
        assert_eq!(points[0].value_base, dec!(1000));
        assert_eq!(points[0].included_count, 1);
        assert!(!points[0].incomplete);
        assert_eq!(points[1].value_base, dec!(1100));
    }

    #[test]
    fn value_history_carries_price_and_fx_forward() {
        let inst = vh_instrument(
            "USD",
            vec![buy(1, d(2026, 1, 2), 10, dec!(100), Some(dec!(10)), "USD")],
            vec![price(d(2026, 1, 2), dec!(100), "USD")],
            vec![
                fx(d(2026, 1, 2), dec!(10), "USD", "SEK"),
                fx(d(2026, 1, 6), dec!(11), "USD", "SEK"),
            ],
        );
        let points = build_value_history(&[inst], None, None).expect("derivable ledger");
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].date, d(2026, 1, 2));
        assert_eq!(points[0].value_base, dec!(10000));
        assert_eq!(points[1].date, d(2026, 1, 6));
        assert_eq!(points[1].value_base, dec!(11000));
    }

    #[test]
    fn value_history_ignores_fx_for_other_pairs() {
        let inst = vh_instrument(
            "USD",
            vec![buy(1, d(2026, 1, 2), 10, dec!(100), Some(dec!(10)), "USD")],
            vec![price(d(2026, 1, 2), dec!(100), "USD")],
            vec![
                fx(d(2026, 1, 2), dec!(10), "EUR", "SEK"),
                fx(d(2026, 1, 3), dec!(11), "USD", "NOK"),
            ],
        );
        let points = build_value_history(&[inst], None, None).expect("derivable ledger");
        assert!(points.is_empty());
    }

    #[test]
    fn value_history_split_adjusts_pre_split_points() {
        let inst = vh_instrument(
            "SEK",
            vec![
                buy(1, d(2026, 1, 2), 10, dec!(120), Some(dec!(1)), "SEK"),
                split(2, d(2026, 1, 10), 10),
            ],
            vec![
                price(d(2026, 1, 5), dec!(60), "SEK"),
                price(d(2026, 1, 12), dec!(60), "SEK"),
            ],
            vec![],
        );
        let points = build_value_history(&[inst], None, None).expect("derivable ledger");
        let p5 = points
            .iter()
            .find(|p| p.date == d(2026, 1, 5))
            .expect("1/5");
        assert_eq!(p5.value_base, dec!(1200));
        let p12 = points
            .iter()
            .find(|p| p.date == d(2026, 1, 12))
            .expect("1/12");
        assert_eq!(p12.value_base, dec!(1200));
    }

    #[test]
    fn value_history_excludes_instrument_with_disabled_mapping() {
        let inst = vh_instrument(
            "SEK",
            vec![buy(1, d(2026, 1, 2), 10, dec!(100), Some(dec!(1)), "SEK")],
            vec![],
            vec![],
        );
        let points = build_value_history(&[inst], None, None).expect("derivable ledger");
        assert!(points.is_empty());
    }

    #[test]
    fn value_history_marks_incomplete_and_omits_all_excluded_dates() {
        let present = vh_instrument(
            "SEK",
            vec![buy(1, d(2026, 1, 2), 10, dec!(100), Some(dec!(1)), "SEK")],
            vec![price(d(2026, 1, 2), dec!(100), "SEK")],
            vec![],
        );
        let absent = vh_instrument(
            "SEK",
            vec![buy(2, d(2026, 1, 2), 5, dec!(50), Some(dec!(1)), "SEK")],
            vec![price(d(2026, 1, 9), dec!(50), "SEK")],
            vec![],
        );
        let points =
            build_value_history(&[present, absent], None, None).expect("derivable ledgers");
        let p2 = points
            .iter()
            .find(|p| p.date == d(2026, 1, 2))
            .expect("1/2");
        assert_eq!(p2.value_base, dec!(1000));
        assert_eq!(p2.included_count, 1);
        assert_eq!(p2.excluded_count, 1);
        assert!(p2.incomplete);
        let p9 = points
            .iter()
            .find(|p| p.date == d(2026, 1, 9))
            .expect("1/9");
        assert_eq!(p9.included_count, 2);
        assert!(!p9.incomplete);
    }

    #[test]
    fn value_history_omits_points_where_every_position_is_excluded() {
        let inst = vh_instrument(
            "USD",
            vec![buy(1, d(2026, 1, 2), 10, dec!(100), Some(dec!(10)), "USD")],
            vec![price(d(2026, 1, 2), dec!(100), "USD")],
            vec![],
        );
        let points = build_value_history(&[inst], None, None).expect("derivable ledger");
        assert!(points.is_empty());
    }

    #[test]
    fn value_history_empty_when_no_buy_yet() {
        let points = build_value_history(&[], None, None).expect("no ledger is Ok(empty)");
        assert!(points.is_empty());
    }

    #[test]
    fn value_history_windows_with_from_and_to() {
        let inst = vh_instrument(
            "SEK",
            vec![buy(1, d(2026, 1, 2), 10, dec!(100), Some(dec!(1)), "SEK")],
            vec![
                price(d(2026, 1, 2), dec!(100), "SEK"),
                price(d(2026, 1, 5), dec!(110), "SEK"),
                price(d(2026, 1, 9), dec!(120), "SEK"),
            ],
            vec![],
        );
        let points = build_value_history(&[inst], Some(d(2026, 1, 5)), Some(d(2026, 1, 5)))
            .expect("derivable ledger");
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].date, d(2026, 1, 5));
        assert_eq!(points[0].value_base, dec!(1100));
    }

    #[test]
    fn build_price_history_carries_fx_forward() {
        // FX only on the 10th; the 11th (no same-day rate) carries the 10th forward.
        let prices = vec![
            price(d(2026, 6, 10), dec!(100), "USD"),
            price(d(2026, 6, 11), dec!(110), "USD"),
        ];
        let fx_rates = vec![fx(d(2026, 6, 10), dec!(10), "USD", "SEK")];

        let points = build_price_history("USD", &prices, &fx_rates);

        assert_eq!(points.len(), 2);
        assert_eq!(points[0].close_base, Availability::available(dec!(1000)));
        assert_eq!(
            points[0].fx,
            Some(FxApplied {
                rate: dec!(10),
                date: d(2026, 6, 10)
            })
        );
        // Carry-forward: the 11th still applies the 10th's rate and reports the 10th's date.
        assert_eq!(points[1].close_base, Availability::available(dec!(1100)));
        assert_eq!(
            points[1].fx,
            Some(FxApplied {
                rate: dec!(10),
                date: d(2026, 6, 10)
            })
        );
    }

    #[test]
    fn build_price_history_marks_missing_fx_before_any_rate() {
        // Price predates every FX rate: close_base unavailable, native close retained, fx omitted.
        let prices = vec![price(d(2026, 6, 9), dec!(100), "USD")];
        let fx_rates = vec![fx(d(2026, 6, 10), dec!(10), "USD", "SEK")];

        let points = build_price_history("USD", &prices, &fx_rates);

        assert_eq!(points.len(), 1);
        assert_eq!(points[0].close, dec!(100));
        assert_eq!(
            points[0].close_base,
            Availability::unavailable(ValuationReason::MissingFx)
        );
        assert_eq!(points[0].fx, None);
    }

    #[test]
    fn build_price_history_carries_fx_dated_before_the_first_price() {
        // The only applicable rate predates the first price: prove the full FX set was used.
        let prices = vec![price(d(2026, 6, 20), dec!(100), "USD")];
        let fx_rates = vec![fx(d(2026, 6, 1), dec!(10), "USD", "SEK")];

        let points = build_price_history("USD", &prices, &fx_rates);

        assert_eq!(points.len(), 1);
        assert_eq!(points[0].close_base, Availability::available(dec!(1000)));
        assert_eq!(
            points[0].fx,
            Some(FxApplied {
                rate: dec!(10),
                date: d(2026, 6, 1)
            })
        );
    }

    #[test]
    fn build_price_history_sek_uses_identity_and_omits_fx() {
        let prices = vec![price(d(2026, 6, 10), dec!(42), "SEK")];

        let points = build_price_history("SEK", &prices, &[]);

        assert_eq!(points.len(), 1);
        assert_eq!(points[0].close_base, Availability::available(dec!(42)));
        assert_eq!(points[0].fx, None);
    }

    #[test]
    fn build_price_history_drops_wrong_currency_rows() {
        // Instrument is USD; a stray EUR row is an internal data error and is excluded.
        let prices = vec![
            price(d(2026, 6, 10), dec!(100), "USD"),
            price(d(2026, 6, 11), dec!(200), "EUR"),
        ];
        let fx_rates = vec![fx(d(2026, 6, 10), dec!(10), "USD", "SEK")];

        let points = build_price_history("USD", &prices, &fx_rates);

        assert_eq!(points.len(), 1);
        assert_eq!(points[0].date, d(2026, 6, 10));
    }

    #[test]
    fn usd_market_value_uses_fx_and_previous_points() {
        let pos = position(&[buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(10)), "USD")]);

        let value = value_position(
            &pos,
            "USD",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(110), "USD")),
            Some(price(d(2026, 6, 13), dec!(100), "USD")),
            Some(fx(d(2026, 6, 16), dec!(10.5), "USD", "SEK")),
            Some(fx(d(2026, 6, 13), dec!(10.0), "USD", "SEK")),
        );

        assert_eq!(
            value.market_value_native,
            Availability::available(dec!(1100))
        );
        assert_eq!(
            value.market_value_base,
            Availability::available(dec!(11550))
        );
        assert_eq!(
            value.unrealized_gain_base,
            Availability::available(dec!(1550))
        );
        assert_eq!(value.price_effect_base, Availability::available(dec!(1050)));
        assert_eq!(value.fx_effect_base, Availability::available(dec!(500)));
        assert_eq!(
            value
                .unrealized_gain_percent
                .as_ref()
                .expect("gain percent")
                .round_dp(2),
            dec!(15.50)
        );
        assert_eq!(value.day_change_base, Availability::available(dec!(1550)));
        assert_eq!(
            value
                .day_change_percent
                .as_ref()
                .expect("day change")
                .round_dp(2),
            dec!(15.50)
        );
    }

    #[test]
    fn older_previous_points_do_not_emit_stale_reasons() {
        let pos = position(&[buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(10)), "USD")]);

        let value = value_position(
            &pos,
            "USD",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(110), "USD")),
            Some(price(d(2026, 6, 10), dec!(100), "USD")),
            Some(fx(d(2026, 6, 16), dec!(10), "USD", "SEK")),
            Some(fx(d(2026, 6, 10), dec!(9), "USD", "SEK")),
        );

        assert!(!value
            .reasons
            .iter()
            .any(|reason| matches!(reason, ValuationReason::StalePrice { .. })));
        assert!(!value
            .reasons
            .iter()
            .any(|reason| matches!(reason, ValuationReason::StaleFx { .. })));
    }

    #[test]
    fn eur_holdings_mark_stale_prices_but_keep_values_visible() {
        let pos = position(&[buy(1, d(2026, 6, 1), 5, dec!(600), Some(dec!(11)), "EUR")]);

        let value = value_position(
            &pos,
            "EUR",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 10), dec!(650), "EUR")),
            Some(price(d(2026, 6, 9), dec!(640), "EUR")),
            Some(fx(d(2026, 6, 10), dec!(11.2), "EUR", "SEK")),
            Some(fx(d(2026, 6, 9), dec!(11.1), "EUR", "SEK")),
        );

        match value.latest_price {
            Availability::Available(snapshot) => {
                assert_eq!(
                    snapshot.freshness,
                    DataFreshness::WarningStale { trading_days: 4 }
                );
            }
            Availability::Unavailable { .. } => panic!("latest price should be available"),
        }
        assert!(value
            .reasons
            .contains(&ValuationReason::StalePrice { trading_days: 4 }));
        assert_eq!(
            value.market_value_base,
            Availability::available(dec!(36400))
        );
    }

    #[test]
    fn sek_positions_use_identity_fx() {
        let pos = position(&[buy(1, d(2026, 6, 1), 4, dec!(20), Some(dec!(1)), "SEK")]);

        let value = value_position(
            &pos,
            "SEK",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(21), "SEK")),
            Some(price(d(2026, 6, 13), dec!(19), "SEK")),
            None,
            None,
        );

        assert_eq!(value.market_value_base, Availability::available(dec!(84)));
        assert_eq!(value.day_change_base, Availability::available(dec!(8)));
        assert_eq!(value.price_effect_base, Availability::available(dec!(4)));
        assert_eq!(value.fx_effect_base, Availability::available(dec!(0)));
        match value.latest_fx {
            Availability::Available(snapshot) => {
                assert_eq!(snapshot.rate, Decimal::ONE);
                assert_eq!(snapshot.base, "SEK");
                assert_eq!(snapshot.quote, "SEK");
            }
            Availability::Unavailable { .. } => panic!("identity fx should be available"),
        }
        match value.previous_fx {
            Availability::Available(snapshot) => {
                assert_eq!(snapshot.rate, Decimal::ONE);
            }
            Availability::Unavailable { .. } => panic!("identity fx should be available"),
        }
    }

    #[test]
    fn missing_price_keeps_cost_basis_but_blocks_market_value() {
        let pos = position(&[buy(1, d(2026, 6, 1), 5, dec!(100), Some(dec!(10)), "USD")]);

        let value = value_position(&pos, "USD", d(2026, 6, 16), None, None, None, None);

        assert_eq!(
            value.market_value_native,
            Availability::Unavailable {
                reasons: vec![ValuationReason::MissingPrice]
            }
        );
        assert_eq!(
            value.market_value_base,
            Availability::Unavailable {
                reasons: vec![ValuationReason::MissingPrice, ValuationReason::MissingFx]
            }
        );
        assert!(value.reasons.contains(&ValuationReason::MissingPrice));
        assert_eq!(value.cost_basis_base, Availability::available(dec!(5000)));
    }

    #[test]
    fn missing_fx_blocks_base_value_for_non_sek_positions() {
        let pos = position(&[buy(1, d(2026, 6, 1), 5, dec!(100), Some(dec!(10)), "USD")]);

        let value = value_position(
            &pos,
            "USD",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(110), "USD")),
            Some(price(d(2026, 6, 13), dec!(100), "USD")),
            None,
            None,
        );

        assert_eq!(
            value.market_value_native,
            Availability::available(dec!(550))
        );
        assert_eq!(
            value.market_value_base,
            Availability::Unavailable {
                reasons: vec![ValuationReason::MissingFx]
            }
        );
        assert_eq!(
            value.price_effect_base,
            Availability::Unavailable {
                reasons: vec![ValuationReason::MissingFx]
            }
        );
        assert_eq!(
            value.fx_effect_base,
            Availability::Unavailable {
                reasons: vec![ValuationReason::MissingFx]
            }
        );
        assert!(value.reasons.contains(&ValuationReason::MissingFx));
    }

    #[test]
    fn base_cost_unavailability_is_propagated() {
        let pos = position(&[
            buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(10)), "USD"),
            buy(2, d(2026, 6, 2), 10, dec!(200), None, "USD"),
        ]);

        let value = value_position(
            &pos,
            "USD",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(110), "USD")),
            Some(price(d(2026, 6, 13), dec!(100), "USD")),
            Some(fx(d(2026, 6, 16), dec!(10), "USD", "SEK")),
            Some(fx(d(2026, 6, 13), dec!(10), "USD", "SEK")),
        );

        assert_eq!(
            value.cost_basis_base,
            Availability::Unavailable {
                reasons: vec![ValuationReason::BaseCostBasisUnavailable {
                    reasons: vec![UnavailableReason::MissingFx { transaction_id: 2 }],
                }]
            }
        );
        assert!(matches!(
            value.unrealized_gain_base,
            Availability::Unavailable { .. }
        ));
        assert_eq!(
            value.price_effect_base,
            Availability::Unavailable {
                reasons: vec![ValuationReason::BaseCostBasisUnavailable {
                    reasons: vec![UnavailableReason::MissingFx { transaction_id: 2 }],
                }]
            }
        );
        assert_eq!(
            value.fx_effect_base,
            Availability::Unavailable {
                reasons: vec![ValuationReason::BaseCostBasisUnavailable {
                    reasons: vec![UnavailableReason::MissingFx { transaction_id: 2 }],
                }]
            }
        );
    }

    #[test]
    fn missing_previous_close_blocks_day_change() {
        let pos = position(&[buy(1, d(2026, 6, 1), 5, dec!(100), Some(dec!(10)), "USD")]);

        let value = value_position(
            &pos,
            "USD",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(110), "USD")),
            None,
            Some(fx(d(2026, 6, 16), dec!(10), "USD", "SEK")),
            Some(fx(d(2026, 6, 13), dec!(10), "USD", "SEK")),
        );

        assert_eq!(
            value.day_change_base,
            Availability::Unavailable {
                reasons: vec![ValuationReason::MissingPreviousClose]
            }
        );
        assert!(value
            .reasons
            .contains(&ValuationReason::MissingPreviousClose));
    }

    #[test]
    fn missing_previous_fx_is_preserved_as_day_change_reason() {
        let pos = position(&[buy(1, d(2026, 6, 1), 5, dec!(100), Some(dec!(10)), "USD")]);

        let value = value_position(
            &pos,
            "USD",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(110), "USD")),
            Some(price(d(2026, 6, 13), dec!(100), "USD")),
            Some(fx(d(2026, 6, 16), dec!(10), "USD", "SEK")),
            None,
        );

        assert_eq!(
            value.day_change_base,
            Availability::Unavailable {
                reasons: vec![ValuationReason::MissingPreviousFx]
            }
        );
        assert!(value.reasons.contains(&ValuationReason::MissingPreviousFx));
    }

    #[test]
    fn day_change_percent_uses_previous_market_value_denominator() {
        let pos = position(&[buy(1, d(2026, 6, 1), 2, dec!(100), Some(dec!(10)), "USD")]);

        let value = value_position(
            &pos,
            "USD",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(110), "USD")),
            Some(price(d(2026, 6, 13), dec!(100), "USD")),
            Some(fx(d(2026, 6, 16), dec!(10), "USD", "SEK")),
            Some(fx(d(2026, 6, 13), dec!(10), "USD", "SEK")),
        );

        assert_eq!(value.day_change_base, Availability::available(dec!(200)));
        assert_eq!(
            value.day_change_percent,
            Availability::available(dec!(10.0))
        );
    }

    #[test]
    fn zero_previous_market_value_makes_day_change_percent_unavailable() {
        let pos = position(&[buy(1, d(2026, 6, 1), 1, dec!(10), Some(dec!(1)), "SEK")]);

        let value = value_position(
            &pos,
            "SEK",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(10), "SEK")),
            Some(price(d(2026, 6, 13), Decimal::ZERO, "SEK")),
            None,
            None,
        );

        assert_eq!(value.day_change_base, Availability::available(dec!(10)));
        assert_eq!(
            value.day_change_percent,
            Availability::Unavailable {
                reasons: vec![ValuationReason::ZeroPreviousMarketValue]
            }
        );

        let summary = summarize_holdings(&[value]);
        assert_eq!(
            summary.day_change_percent,
            Availability::Unavailable {
                reasons: vec![ValuationReason::ZeroPreviousMarketValue]
            }
        );
    }

    #[test]
    fn zero_cost_basis_makes_gain_percent_unavailable() {
        let pos = Position {
            quantity: 1,
            cost_basis_native: Decimal::ZERO,
            base: BaseCostBasis::Available {
                cost_basis_base: Decimal::ZERO,
                fee_component_base: Decimal::ZERO,
            },
        };
        let value = value_position(
            &pos,
            "SEK",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(10), "SEK")),
            Some(price(d(2026, 6, 13), dec!(9), "SEK")),
            None,
            None,
        );

        assert_eq!(
            value.unrealized_gain_percent,
            Availability::Unavailable {
                reasons: vec![ValuationReason::ZeroCostBasis]
            }
        );
        assert_eq!(
            value.price_effect_base,
            Availability::Unavailable {
                reasons: vec![ValuationReason::ZeroCostBasis]
            }
        );
        assert_eq!(
            value.fx_effect_base,
            Availability::Unavailable {
                reasons: vec![ValuationReason::ZeroCostBasis]
            }
        );
    }

    #[test]
    fn brokerage_lands_in_price_effect_when_price_and_fx_are_unchanged() {
        let mut tx = buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(10)), "USD");
        tx.brokerage_base = dec!(25);
        let pos = position(&[tx]);

        let value = value_position(
            &pos,
            "USD",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(100), "USD")),
            Some(price(d(2026, 6, 13), dec!(100), "USD")),
            Some(fx(d(2026, 6, 16), dec!(10), "USD", "SEK")),
            Some(fx(d(2026, 6, 13), dec!(10), "USD", "SEK")),
        );

        assert_eq!(value.cost_basis_base, Availability::available(dec!(10025)));
        assert_eq!(value.price_effect_base, Availability::available(dec!(-25)));
        assert_eq!(value.fx_effect_base, Availability::available(dec!(0)));
        assert_eq!(
            value.unrealized_gain_base,
            Availability::available(dec!(-25))
        );
    }

    #[test]
    fn multi_lot_buys_at_different_fx_rates_split_gain_across_price_and_fx() {
        let pos = position(&[
            buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(10)), "USD"),
            buy(2, d(2026, 6, 2), 10, dec!(100), Some(dec!(12)), "USD"),
        ]);

        let value = value_position(
            &pos,
            "USD",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(120), "USD")),
            Some(price(d(2026, 6, 13), dec!(100), "USD")),
            Some(fx(d(2026, 6, 16), dec!(13), "USD", "SEK")),
            Some(fx(d(2026, 6, 13), dec!(12), "USD", "SEK")),
        );

        assert_eq!(value.cost_basis_base, Availability::available(dec!(22000)));
        assert_eq!(
            value.market_value_base,
            Availability::available(dec!(31200))
        );
        assert_eq!(
            value.unrealized_gain_base,
            Availability::available(dec!(9200))
        );
        assert_eq!(value.price_effect_base, Availability::available(dec!(5200)));
        assert_eq!(value.fx_effect_base, Availability::available(dec!(4000)));
    }

    #[test]
    fn summary_attribution_sums_match_row_effects_and_total_gain() {
        let usd = value_position(
            &position(&[buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(10)), "USD")]),
            "USD",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(110), "USD")),
            Some(price(d(2026, 6, 13), dec!(100), "USD")),
            Some(fx(d(2026, 6, 16), dec!(11), "USD", "SEK")),
            Some(fx(d(2026, 6, 13), dec!(10), "USD", "SEK")),
        );
        let sek = value_position(
            &position(&[buy(2, d(2026, 6, 1), 5, dec!(20), Some(dec!(1)), "SEK")]),
            "SEK",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(30), "SEK")),
            Some(price(d(2026, 6, 13), dec!(25), "SEK")),
            None,
            None,
        );

        let summary = summarize_holdings(&[usd.clone(), sek.clone()]);

        assert_eq!(
            summary.market_value_base,
            Availability::available(dec!(12250))
        );
        assert_eq!(
            summary.cost_basis_base,
            Availability::available(dec!(10100))
        );
        assert_eq!(
            summary.price_effect_base,
            Availability::available(dec!(1150))
        );
        assert_eq!(summary.fx_effect_base, Availability::available(dec!(1000)));
        assert_eq!(
            summary.unrealized_gain_base,
            Availability::available(dec!(2150))
        );
        assert_eq!(
            *summary.price_effect_base.as_ref().expect("price effect")
                + *summary.fx_effect_base.as_ref().expect("fx effect"),
            *summary.unrealized_gain_base.as_ref().expect("gain")
        );

        assert_eq!(
            *usd.price_effect_base.as_ref().expect("usd price effect")
                + *usd.fx_effect_base.as_ref().expect("usd fx effect"),
            *usd.unrealized_gain_base.as_ref().expect("usd gain")
        );
        assert_eq!(
            *sek.price_effect_base.as_ref().expect("sek price effect")
                + *sek.fx_effect_base.as_ref().expect("sek fx effect"),
            *sek.unrealized_gain_base.as_ref().expect("sek gain")
        );
    }
    #[test]
    fn weekday_staleness_counts_public_holidays_as_trading_days_for_now() {
        assert_eq!(
            super::data_freshness(d(2026, 6, 22), d(2026, 6, 18)),
            DataFreshness::MinorStale { trading_days: 2 }
        );
    }

    #[test]
    fn split_adjusted_quantity_combines_with_current_price() {
        let pos = position(&[
            buy(1, d(2026, 6, 1), 10, dec!(120), Some(dec!(1)), "USD"),
            split(2, d(2026, 6, 2), 10),
        ]);

        let value = value_position(
            &pos,
            "USD",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(60), "USD")),
            Some(price(d(2026, 6, 13), dec!(55), "USD")),
            Some(fx(d(2026, 6, 16), dec!(1), "USD", "SEK")),
            Some(fx(d(2026, 6, 13), dec!(1), "USD", "SEK")),
        );

        assert_eq!(value.quantity, 20);
        assert_eq!(value.cost_basis_native, dec!(1200));
        assert_eq!(
            value.market_value_native,
            Availability::available(dec!(1200))
        );
    }

    #[test]
    fn summary_excludes_incomplete_rows() {
        let included = value_position(
            &position(&[buy(1, d(2026, 6, 1), 5, dec!(100), Some(dec!(10)), "USD")]),
            "USD",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(110), "USD")),
            Some(price(d(2026, 6, 13), dec!(100), "USD")),
            Some(fx(d(2026, 6, 16), dec!(10), "USD", "SEK")),
            Some(fx(d(2026, 6, 13), dec!(10), "USD", "SEK")),
        );
        let excluded = value_position(
            &position(&[buy(2, d(2026, 6, 1), 5, dec!(100), Some(dec!(10)), "USD")]),
            "USD",
            d(2026, 6, 16),
            None,
            None,
            None,
            None,
        );

        let summary = summarize_holdings(&[included, excluded]);

        assert_eq!(summary.excluded_rows, 1);
        assert_eq!(
            summary.market_value_base,
            Availability::available(dec!(5500))
        );
        assert_eq!(summary.cost_basis_base, Availability::available(dec!(5000)));
        assert_eq!(
            summary.unrealized_gain_base,
            Availability::available(dec!(500))
        );
        assert_eq!(
            summary.price_effect_base,
            Availability::available(dec!(500))
        );
        assert_eq!(summary.fx_effect_base, Availability::available(dec!(0)));
    }
}
