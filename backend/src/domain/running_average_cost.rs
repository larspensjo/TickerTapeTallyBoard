use chrono::NaiveDate;
use rust_decimal::Decimal;

use super::performance::split_factor;
use super::position::fold_positions;
use super::transaction::{LedgerError, LedgerTransaction, TransactionKind};

/// The running weighted-average native cost per share over an instrument's
/// ledger, one segment per held period.
#[derive(Clone, Debug, PartialEq)]
pub struct RunningAverageCost {
    pub segments: Vec<AverageCostSegment>,
    /// The open position's native average; `None` when nothing is held.
    pub current_average_native: Option<Decimal>,
}

/// One held period: steps in ascending date order, closed on the first trade
/// date that ended with nothing held, or `None` while still open.
#[derive(Clone, Debug, PartialEq)]
pub struct AverageCostSegment {
    pub steps: Vec<AverageCostStep>,
    pub closed_on: Option<NaiveDate>,
}

/// The split-adjusted native average at the end of a buy day.
#[derive(Clone, Debug, PartialEq)]
pub struct AverageCostStep {
    pub date: NaiveDate,
    pub average_native: Decimal,
}

/// End-of-day position state for one trade date.
struct DayEnd {
    date: NaiveDate,
    had_buy: bool,
    quantity: i64,
    cost_basis_native: Decimal,
}

/// Derive the running average cost from `(trade_date, id)`-ordered
/// transactions at daily resolution.
///
/// A step is emitted for every trade date with a Buy that ends with shares
/// held; a segment closes on the first trade date that ends flat. Each step is
/// split-adjusted by the ledger's later splits, and an open final segment ends
/// on the derived position's native average exactly.
pub fn running_average_cost(
    ledger: &[LedgerTransaction],
) -> Result<RunningAverageCost, LedgerError> {
    let mut days: Vec<DayEnd> = Vec::new();
    let final_position = fold_positions(ledger, |tx, position| {
        let is_buy = tx.kind == TransactionKind::Buy;
        match days.last_mut() {
            Some(day) if day.date == tx.trade_date => {
                day.had_buy |= is_buy;
                day.quantity = position.quantity;
                day.cost_basis_native = position.cost_basis_native;
            }
            _ => days.push(DayEnd {
                date: tx.trade_date,
                had_buy: is_buy,
                quantity: position.quantity,
                cost_basis_native: position.cost_basis_native,
            }),
        }
    })?;

    let mut segments: Vec<AverageCostSegment> = Vec::new();
    let mut open: Option<Vec<AverageCostStep>> = None;
    for day in &days {
        if day.quantity == 0 {
            if let Some(steps) = open.take() {
                segments.push(AverageCostSegment {
                    steps,
                    closed_on: Some(day.date),
                });
            }
            continue;
        }
        if !day.had_buy {
            continue;
        }

        // The ledger is date-ordered, so rows dated after this day are a suffix.
        let later = &ledger[ledger.partition_point(|tx| tx.trade_date <= day.date)..];
        let factor = split_factor(later, day.quantity)?;
        let raw_average = day.cost_basis_native / Decimal::from(day.quantity);
        open.get_or_insert_with(Vec::new).push(AverageCostStep {
            date: day.date,
            average_native: raw_average / factor,
        });
    }

    let current_average_native = match open {
        Some(mut steps) => {
            let average = final_position.average_cost_native();
            debug_assert!(average.is_some(), "open segment requires an open position");
            if let (Some(last), Some(average)) = (steps.last_mut(), average) {
                last.average_native = average;
            }
            segments.push(AverageCostSegment {
                steps,
                closed_on: None,
            });
            average
        }
        None => None,
    };

    Ok(RunningAverageCost {
        segments,
        current_average_native,
    })
}

#[cfg(test)]
mod tests {
    use super::{running_average_cost, AverageCostSegment, AverageCostStep, RunningAverageCost};
    use crate::domain::position::derive_position;
    use crate::domain::transaction::{LedgerError, LedgerTransaction, TransactionKind};
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

    fn sell(id: i64, date: NaiveDate, qty: i64) -> LedgerTransaction {
        LedgerTransaction {
            id,
            trade_date: date,
            kind: TransactionKind::Sell,
            quantity: -qty,
            price: Some(dec!(1)),
            dividend_per_share: None,
            fx_rate_to_base: None,
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

    fn dividend(id: i64, date: NaiveDate) -> LedgerTransaction {
        LedgerTransaction {
            id,
            trade_date: date,
            kind: TransactionKind::Dividend,
            quantity: 0,
            price: None,
            dividend_per_share: Some(dec!(2)),
            fx_rate_to_base: Some(dec!(1)),
            brokerage_base: Decimal::ZERO,
        }
    }

    fn step(date: NaiveDate, average_native: Decimal) -> AverageCostStep {
        AverageCostStep {
            date,
            average_native,
        }
    }

    fn derive(ledger: &[LedgerTransaction]) -> RunningAverageCost {
        running_average_cost(ledger).expect("derives")
    }

    fn single_buy() -> Vec<LedgerTransaction> {
        vec![buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(1)))]
    }

    fn two_buys() -> Vec<LedgerTransaction> {
        vec![
            buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(1))),
            buy(2, d(2026, 6, 5), 30, dec!(120), Some(dec!(1))),
        ]
    }

    fn partial_sell() -> Vec<LedgerTransaction> {
        vec![
            buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(1))),
            sell(2, d(2026, 6, 3), 4),
        ]
    }

    fn full_close() -> Vec<LedgerTransaction> {
        vec![
            buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(1))),
            sell(2, d(2026, 6, 3), 4),
            sell(3, d(2026, 6, 8), 6),
            dividend(4, d(2026, 6, 20)),
        ]
    }

    fn close_then_reopen() -> Vec<LedgerTransaction> {
        vec![
            buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(1))),
            sell(2, d(2026, 6, 8), 10),
            buy(3, d(2026, 7, 1), 5, dec!(80), Some(dec!(1))),
        ]
    }

    fn same_day_close_and_reopen() -> Vec<LedgerTransaction> {
        vec![
            buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(1))),
            sell(2, d(2026, 6, 8), 10),
            buy(3, d(2026, 6, 8), 5, dec!(80), Some(dec!(1))),
        ]
    }

    fn several_same_day_buys() -> Vec<LedgerTransaction> {
        vec![
            buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(1))),
            buy(2, d(2026, 6, 1), 10, dec!(200), Some(dec!(1))),
            buy(3, d(2026, 6, 1), 20, dec!(120), Some(dec!(1))),
        ]
    }

    fn same_day_buy_then_partial_sell() -> Vec<LedgerTransaction> {
        vec![
            buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(1))),
            buy(2, d(2026, 6, 2), 10, dec!(200), Some(dec!(1))),
            sell(3, d(2026, 6, 2), 5),
        ]
    }

    fn same_day_round_trip() -> Vec<LedgerTransaction> {
        vec![
            buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(1))),
            sell(2, d(2026, 6, 1), 10),
        ]
    }

    fn buy_on_closing_day() -> Vec<LedgerTransaction> {
        vec![
            buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(1))),
            buy(2, d(2026, 6, 8), 5, dec!(120), Some(dec!(1))),
            sell(3, d(2026, 6, 8), 15),
        ]
    }

    fn split_after_last_buy() -> Vec<LedgerTransaction> {
        vec![
            buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(1))),
            buy(2, d(2026, 6, 5), 10, dec!(140), Some(dec!(1))),
            split(3, d(2026, 6, 10), 60),
        ]
    }

    /// Three shares consolidate into two: the split factor 2/3 repeats.
    fn three_to_two_split() -> Vec<LedgerTransaction> {
        vec![
            buy(1, d(2026, 6, 1), 2, dec!(10), Some(dec!(1))),
            buy(2, d(2026, 6, 5), 1, dec!(11), Some(dec!(1))),
            split(3, d(2026, 6, 10), -1),
        ]
    }

    /// Five shares for every three: the split factor 5/3 repeats.
    fn five_for_three_split() -> Vec<LedgerTransaction> {
        vec![
            buy(1, d(2026, 6, 1), 3, dec!(10), Some(dec!(1))),
            buy(2, d(2026, 6, 5), 3, dec!(11), Some(dec!(1))),
            split(3, d(2026, 6, 10), 4),
        ]
    }

    fn split_between_buys() -> Vec<LedgerTransaction> {
        vec![
            buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(1))),
            split(2, d(2026, 6, 5), 10),
            buy(3, d(2026, 6, 10), 20, dec!(60), Some(dec!(1))),
        ]
    }

    fn sell_between_buy_and_split() -> Vec<LedgerTransaction> {
        vec![
            buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(1))),
            sell(2, d(2026, 6, 3), 5),
            split(3, d(2026, 6, 5), 5),
            buy(4, d(2026, 6, 10), 10, dec!(70), Some(dec!(1))),
        ]
    }

    fn split_in_later_held_period() -> Vec<LedgerTransaction> {
        vec![
            buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(1))),
            sell(2, d(2026, 6, 8), 10),
            buy(3, d(2026, 7, 1), 10, dec!(40), Some(dec!(1))),
            split(4, d(2026, 7, 10), 10),
        ]
    }

    fn mixed_fx() -> Vec<LedgerTransaction> {
        vec![
            buy(1, d(2026, 6, 1), 10, dec!(100), None),
            buy(2, d(2026, 6, 5), 10, dec!(200), Some(dec!(3.5))),
        ]
    }

    fn with_dividends() -> Vec<LedgerTransaction> {
        vec![
            buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(1))),
            dividend(2, d(2026, 6, 3)),
            buy(3, d(2026, 6, 5), 10, dec!(200), Some(dec!(1))),
            dividend(4, d(2026, 6, 5)),
            dividend(5, d(2026, 6, 9)),
        ]
    }

    fn all_fixtures() -> Vec<Vec<LedgerTransaction>> {
        vec![
            Vec::new(),
            single_buy(),
            two_buys(),
            partial_sell(),
            full_close(),
            close_then_reopen(),
            same_day_close_and_reopen(),
            several_same_day_buys(),
            same_day_buy_then_partial_sell(),
            same_day_round_trip(),
            buy_on_closing_day(),
            split_after_last_buy(),
            three_to_two_split(),
            five_for_three_split(),
            split_between_buys(),
            sell_between_buy_and_split(),
            split_in_later_held_period(),
            mixed_fx(),
            with_dividends(),
        ]
    }

    #[test]
    fn single_buy_opens_one_segment_at_the_buy_price() {
        let cost = derive(&single_buy());

        assert_eq!(
            cost.segments,
            vec![AverageCostSegment {
                steps: vec![step(d(2026, 6, 1), dec!(100))],
                closed_on: None,
            }]
        );
        assert_eq!(cost.current_average_native, Some(dec!(100)));
    }

    #[test]
    fn second_buy_steps_to_the_weighted_average() {
        let cost = derive(&two_buys());

        assert_eq!(
            cost.segments,
            vec![AverageCostSegment {
                steps: vec![
                    step(d(2026, 6, 1), dec!(100)),
                    step(d(2026, 6, 5), dec!(115))
                ],
                closed_on: None,
            }]
        );
    }

    #[test]
    fn partial_sell_adds_no_step_and_keeps_the_average() {
        let cost = derive(&partial_sell());

        assert_eq!(
            cost.segments,
            vec![AverageCostSegment {
                steps: vec![step(d(2026, 6, 1), dec!(100))],
                closed_on: None,
            }]
        );
        assert_eq!(cost.current_average_native, Some(dec!(100)));
    }

    #[test]
    fn full_close_ends_the_segment_on_the_closing_sell_date() {
        let cost = derive(&full_close());

        assert_eq!(
            cost.segments,
            vec![AverageCostSegment {
                steps: vec![step(d(2026, 6, 1), dec!(100))],
                closed_on: Some(d(2026, 6, 8)),
            }]
        );
        assert_eq!(cost.current_average_native, None);
    }

    #[test]
    fn reopen_on_a_later_day_starts_a_fresh_segment_at_the_reopen_price() {
        let cost = derive(&close_then_reopen());

        assert_eq!(
            cost.segments,
            vec![
                AverageCostSegment {
                    steps: vec![step(d(2026, 6, 1), dec!(100))],
                    closed_on: Some(d(2026, 6, 8)),
                },
                AverageCostSegment {
                    steps: vec![step(d(2026, 7, 1), dec!(80))],
                    closed_on: None,
                },
            ]
        );
        assert_eq!(cost.current_average_native, Some(dec!(80)));
    }

    #[test]
    fn same_day_close_and_reopen_continues_the_segment() {
        let cost = derive(&same_day_close_and_reopen());

        assert_eq!(
            cost.segments,
            vec![AverageCostSegment {
                steps: vec![
                    step(d(2026, 6, 1), dec!(100)),
                    step(d(2026, 6, 8), dec!(80))
                ],
                closed_on: None,
            }]
        );
    }

    #[test]
    fn several_same_day_buys_emit_one_end_of_day_step() {
        let cost = derive(&several_same_day_buys());

        assert_eq!(
            cost.segments,
            vec![AverageCostSegment {
                steps: vec![step(d(2026, 6, 1), dec!(135))],
                closed_on: None,
            }]
        );
    }

    #[test]
    fn same_day_buy_then_partial_sell_emits_the_end_of_day_average() {
        let cost = derive(&same_day_buy_then_partial_sell());

        assert_eq!(
            cost.segments,
            vec![AverageCostSegment {
                steps: vec![
                    step(d(2026, 6, 1), dec!(100)),
                    step(d(2026, 6, 2), dec!(150))
                ],
                closed_on: None,
            }]
        );
    }

    #[test]
    fn same_day_round_trip_from_flat_produces_no_segment() {
        let cost = derive(&same_day_round_trip());

        assert!(cost.segments.is_empty());
        assert_eq!(cost.current_average_native, None);
    }

    #[test]
    fn buy_on_the_closing_day_adds_no_step() {
        let cost = derive(&buy_on_closing_day());

        assert_eq!(
            cost.segments,
            vec![AverageCostSegment {
                steps: vec![step(d(2026, 6, 1), dec!(100))],
                closed_on: Some(d(2026, 6, 8)),
            }]
        );
    }

    #[test]
    fn split_after_the_last_buy_adjusts_earlier_steps() {
        let ledger = split_after_last_buy();
        let cost = derive(&ledger);
        let position = derive_position(&ledger).expect("derives");

        // 20 shares become 80: factor 4.
        let steps = &cost.segments[0].steps;
        assert_eq!(steps[0], step(d(2026, 6, 1), dec!(25)));
        assert_eq!(steps[1], step(d(2026, 6, 5), dec!(30)));
        assert_eq!(
            Some(steps[1].average_native),
            position.average_cost_native()
        );
    }

    #[test]
    fn repeating_split_factor_ends_exactly_on_the_position_average() {
        let ledger = three_to_two_split();
        let cost = derive(&ledger);
        let position = derive_position(&ledger).expect("derives");

        // Factor 2/3: 10 / (2/3) = 15; final average 31 / 2 = 15.5.
        let steps = &cost.segments[0].steps;
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].average_native.round_dp(10), dec!(15));
        assert_eq!(
            Some(steps[1].average_native),
            position.average_cost_native()
        );
        assert_eq!(steps[1].average_native, dec!(15.5));
        assert_eq!(cost.current_average_native, Some(dec!(15.5)));
    }

    #[test]
    fn five_for_three_split_ends_exactly_on_the_position_average() {
        let ledger = five_for_three_split();
        let cost = derive(&ledger);
        let position = derive_position(&ledger).expect("derives");

        // Factor 5/3: 10 / (5/3) = 6; final average 63 / 10 = 6.3.
        let steps = &cost.segments[0].steps;
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].average_native.round_dp(10), dec!(6));
        assert_eq!(
            Some(steps[1].average_native),
            position.average_cost_native()
        );
        assert_eq!(steps[1].average_native, dec!(6.3));
        assert_eq!(cost.current_average_native, Some(dec!(6.3)));
    }

    #[test]
    fn split_between_buys_adjusts_only_the_earlier_step() {
        let cost = derive(&split_between_buys());

        // 10 shares become 20 before the second buy: factor 2.
        assert_eq!(
            cost.segments,
            vec![AverageCostSegment {
                steps: vec![
                    step(d(2026, 6, 1), dec!(50)),
                    step(d(2026, 6, 10), dec!(55))
                ],
                closed_on: None,
            }]
        );
    }

    #[test]
    fn split_factor_uses_the_running_quantity_at_the_split() {
        let cost = derive(&sell_between_buy_and_split());

        // The sell leaves 5 shares, the split makes them 10: factor 2, not 1.5.
        assert_eq!(
            cost.segments,
            vec![AverageCostSegment {
                steps: vec![
                    step(d(2026, 6, 1), dec!(50)),
                    step(d(2026, 6, 10), dec!(60))
                ],
                closed_on: None,
            }]
        );
    }

    #[test]
    fn split_in_a_later_held_period_adjusts_an_earlier_closed_segment() {
        let cost = derive(&split_in_later_held_period());

        assert_eq!(
            cost.segments,
            vec![
                AverageCostSegment {
                    steps: vec![step(d(2026, 6, 1), dec!(50))],
                    closed_on: Some(d(2026, 6, 8)),
                },
                AverageCostSegment {
                    steps: vec![step(d(2026, 7, 1), dec!(20))],
                    closed_on: None,
                },
            ]
        );
    }

    #[test]
    fn fx_rates_never_enter_the_native_average() {
        let cost = derive(&mixed_fx());

        assert_eq!(
            cost.segments,
            vec![AverageCostSegment {
                steps: vec![
                    step(d(2026, 6, 1), dec!(100)),
                    step(d(2026, 6, 5), dec!(150))
                ],
                closed_on: None,
            }]
        );
    }

    #[test]
    fn dividend_rows_are_ignored() {
        let cost = derive(&with_dividends());

        assert_eq!(
            cost.segments,
            vec![AverageCostSegment {
                steps: vec![
                    step(d(2026, 6, 1), dec!(100)),
                    step(d(2026, 6, 5), dec!(150))
                ],
                closed_on: None,
            }]
        );
    }

    #[test]
    fn every_fixture_satisfies_the_segment_invariants() {
        for ledger in all_fixtures() {
            let cost = derive(&ledger);
            let position = derive_position(&ledger).expect("derives");

            let mut previous_end: Option<NaiveDate> = None;
            for (index, segment) in cost.segments.iter().enumerate() {
                assert!(!segment.steps.is_empty(), "{ledger:?}");
                assert!(
                    segment
                        .steps
                        .windows(2)
                        .all(|pair| pair[0].date < pair[1].date),
                    "{ledger:?}"
                );
                let first = segment.steps[0].date;
                let last = segment.steps[segment.steps.len() - 1].date;
                if let Some(end) = previous_end {
                    assert!(end < first, "{ledger:?}");
                }
                match segment.closed_on {
                    Some(closed_on) => {
                        assert!(last < closed_on, "{ledger:?}");
                        previous_end = Some(closed_on);
                    }
                    None => assert_eq!(index, cost.segments.len() - 1, "{ledger:?}"),
                }
            }

            let open = cost
                .segments
                .last()
                .filter(|segment| segment.closed_on.is_none());
            assert_eq!(open.is_some(), position.quantity > 0, "{ledger:?}");
            assert_eq!(cost.current_average_native.is_some(), open.is_some());
            if let Some(segment) = open {
                let last = segment.steps.last().expect("non-empty");
                assert_eq!(
                    Some(last.average_native),
                    position.average_cost_native(),
                    "{ledger:?}"
                );
                assert_eq!(cost.current_average_native, position.average_cost_native());
            }
        }
    }

    #[test]
    fn sell_exceeding_the_position_propagates_with_its_transaction_id() {
        let ledger = vec![
            buy(1, d(2026, 6, 1), 3, dec!(100), Some(dec!(1))),
            sell(2, d(2026, 6, 2), 4),
        ];

        let error = running_average_cost(&ledger).expect_err("oversell fails");
        assert_eq!(
            error,
            LedgerError::SellExceedsPosition {
                transaction_id: 2,
                available: 3,
                requested: 4,
            }
        );
    }
}
