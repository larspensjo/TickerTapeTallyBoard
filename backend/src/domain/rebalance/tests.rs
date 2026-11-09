//! Tests for the pure rebalance ladder construction.
use super::{RankBy, RebalanceCandidate, RebalanceUnavailable, TradeSide, UntradedReason};
use crate::domain::TargetStatus;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn candidate(
    instrument_id: i64,
    weight: Decimal,
    market_value_base: Decimal,
    price_base: Decimal,
    held_quantity: i64,
) -> RebalanceCandidate {
    RebalanceCandidate {
        instrument_id,
        weight,
        market_value_base,
        price_base,
        held_quantity,
    }
}

fn build_ladder(
    candidates: &[RebalanceCandidate],
    offset: Decimal,
) -> Result<super::RebalanceLadder, RebalanceUnavailable> {
    super::build_ladder(candidates, offset, RankBy::Sek)
}

fn build_percent_ladder(
    candidates: &[RebalanceCandidate],
    offset: Decimal,
) -> Result<super::RebalanceLadder, RebalanceUnavailable> {
    super::build_ladder(candidates, offset, RankBy::Percent)
}

fn ideal_deltas(
    candidates: &[RebalanceCandidate],
    offset: Decimal,
) -> (Decimal, Decimal, Vec<Decimal>) {
    let allocation = super::ideal_deltas(candidates, offset);
    let total_weight = candidates.iter().map(|candidate| candidate.weight).sum();
    (allocation.pool_value_base, total_weight, allocation.deltas)
}

fn ranked_indices(ideal_deltas: &[Decimal]) -> Vec<usize> {
    super::ranked_indices(RankBy::Sek, &[], &[], ideal_deltas)
}

fn build_rung(
    candidates: &[RebalanceCandidate],
    ideal_deltas: &[Decimal],
    offset: Decimal,
    selected_count: usize,
) -> super::RebalanceRung {
    let targets: Vec<Decimal> = candidates
        .iter()
        .zip(ideal_deltas.iter())
        .map(|(candidate, delta)| candidate.market_value_base + *delta)
        .collect();
    let context = super::RungContext {
        candidates,
        deltas: ideal_deltas,
        targets: &targets,
        offset,
        rank_by: RankBy::Sek,
    };
    super::build_rung(&context, selected_count)
}

fn build_percent_rung(
    candidates: &[RebalanceCandidate],
    ideal_deltas: &[Decimal],
    offset: Decimal,
    selected_count: usize,
) -> super::RebalanceRung {
    let targets: Vec<Decimal> = candidates
        .iter()
        .zip(ideal_deltas.iter())
        .map(|(candidate, delta)| candidate.market_value_base + *delta)
        .collect();
    let context = super::RungContext {
        candidates,
        deltas: ideal_deltas,
        targets: &targets,
        offset,
        rank_by: RankBy::Percent,
    };
    super::build_rung(&context, selected_count)
}

fn redistribute_selected(
    candidates: &[RebalanceCandidate],
    selected: &[bool],
    ideal_deltas: &[Decimal],
    offset: Decimal,
) -> super::RedistributionState {
    let targets: Vec<Decimal> = candidates
        .iter()
        .zip(ideal_deltas.iter())
        .map(|(candidate, delta)| candidate.market_value_base + *delta)
        .collect();
    let context = super::RungContext {
        candidates,
        deltas: ideal_deltas,
        targets: &targets,
        offset,
        rank_by: RankBy::Sek,
    };
    super::redistribute_selected(&context, selected)
}

fn redistribute_selected_percent(
    candidates: &[RebalanceCandidate],
    selected: &[bool],
    ideal_deltas: &[Decimal],
    offset: Decimal,
) -> super::RedistributionState {
    let targets: Vec<Decimal> = candidates
        .iter()
        .zip(ideal_deltas.iter())
        .map(|(candidate, delta)| candidate.market_value_base + *delta)
        .collect();
    let context = super::RungContext {
        candidates,
        deltas: ideal_deltas,
        targets: &targets,
        offset,
        rank_by: RankBy::Percent,
    };
    super::redistribute_selected(&context, selected)
}

fn resolve_quantities(
    candidates: &[RebalanceCandidate],
    selected: &[bool],
    ideal_deltas: &[Decimal],
    x: &[Decimal],
    offset: Decimal,
    skip_greedy_pass: bool,
) -> super::QuantityState {
    let targets: Vec<Decimal> = candidates
        .iter()
        .zip(ideal_deltas.iter())
        .map(|(candidate, delta)| candidate.market_value_base + *delta)
        .collect();
    let context = super::RungContext {
        candidates,
        deltas: ideal_deltas,
        targets: &targets,
        offset,
        rank_by: RankBy::Sek,
    };
    super::resolve_quantities(&context, selected, x, skip_greedy_pass)
}

fn resolve_quantities_percent(
    candidates: &[RebalanceCandidate],
    selected: &[bool],
    ideal_deltas: &[Decimal],
    x: &[Decimal],
    offset: Decimal,
    skip_greedy_pass: bool,
) -> super::QuantityState {
    let targets: Vec<Decimal> = candidates
        .iter()
        .zip(ideal_deltas.iter())
        .map(|(candidate, delta)| candidate.market_value_base + *delta)
        .collect();
    let context = super::RungContext {
        candidates,
        deltas: ideal_deltas,
        targets: &targets,
        offset,
        rank_by: RankBy::Percent,
    };
    super::resolve_quantities(&context, selected, x, skip_greedy_pass)
}

fn worked_fixture() -> Vec<RebalanceCandidate> {
    vec![
        candidate(1, dec!(1), dec!(100000), dec!(1000), 100),
        candidate(2, dec!(2), dec!(300000), dec!(1000), 300),
        candidate(3, dec!(4), dec!(300000), dec!(1000), 300),
    ]
}

#[test]
fn ideal_deltas_sum_exactly_to_offset() {
    let candidates = worked_fixture();
    let offset = dec!(12345.67);
    let (pool_value, _, deltas) = ideal_deltas(&candidates, offset);
    let sum: Decimal = deltas.iter().copied().sum();
    assert_eq!(pool_value, dec!(700000));
    assert_eq!(sum, offset);
}

#[test]
fn identical_inputs_produce_identical_ladders() {
    let candidates = worked_fixture();
    let left = build_ladder(&candidates, Decimal::ZERO).expect("ladder");
    let right = build_ladder(&candidates, Decimal::ZERO).expect("ladder");
    assert_eq!(left, right);
}

#[test]
fn mixed_pool_redistributes_into_a_zero_value_buy_only_candidate() {
    let candidates = vec![
        candidate(1, dec!(1), dec!(100), dec!(1), 100),
        candidate(2, dec!(4), dec!(0), dec!(1), 0),
    ];
    let offset = dec!(50);
    let (pool_value, _, deltas) = ideal_deltas(&candidates, offset);
    assert_eq!(pool_value, dec!(100));
    assert_eq!(deltas.iter().copied().sum::<Decimal>(), offset);

    let ladder = build_ladder(&candidates, offset).expect("ladder");
    let rung = &ladder.rungs[0];
    assert_eq!(rung.selected_count, 1);
    assert_eq!(rung.effective_trade_count, 1);
    assert_eq!(rung.trades.len(), 1);
    assert_eq!(rung.trades[0].instrument_id, 2);
    assert_eq!(rung.trades[0].side, TradeSide::Buy);
    assert_eq!(rung.trades[0].shares, 50);
    assert_eq!(rung.achieved_net_base, offset);
    assert_eq!(rung.residual_base, Decimal::ZERO);
    assert!(rung.coverage_percent.expect("coverage") > Decimal::ZERO);

    let gap_before_sum: Decimal = rung.balance.iter().map(|entry| entry.gap_before_base).sum();
    let gap_after_sum: Decimal = rung.balance.iter().map(|entry| entry.gap_after_base).sum();
    assert_eq!(gap_before_sum, -offset);
    assert_eq!(gap_after_sum, -rung.residual_base);
    assert_eq!(
        rung.total_gap_before_base,
        rung.balance
            .iter()
            .map(|entry| entry.gap_before_base.abs())
            .sum::<Decimal>()
    );
    assert_eq!(
        rung.total_gap_after_base,
        rung.balance
            .iter()
            .map(|entry| entry.gap_after_base.abs())
            .sum::<Decimal>()
    );
}

#[test]
fn mixed_pool_keeps_all_zero_fallback_honest_with_a_zero_value_watchlist_member_present() {
    let candidates = vec![
        candidate(1, dec!(4), dec!(100), dec!(1), 100),
        candidate(2, dec!(1), dec!(0), dec!(1), 0),
    ];
    let ladder = build_ladder(&candidates, Decimal::ZERO).expect("ladder");
    let rung = &ladder.rungs[0];

    assert_eq!(rung.selected_count, 1);
    assert_eq!(rung.effective_trade_count, 1);
    assert_eq!(
        rung.trades,
        vec![super::PlannedTrade {
            instrument_id: 1,
            side: TradeSide::Sell,
            shares: 20,
            price_base: dec!(1),
            amount_base: dec!(20),
        }]
    );
    assert_eq!(rung.achieved_net_base, dec!(-20));
    assert_eq!(rung.residual_base, dec!(20));
    assert_eq!(rung.coverage_percent.expect("coverage"), dec!(50));
}

#[test]
fn pure_watchlist_pool_splits_cash_by_conviction_weight_and_produces_buy_ladder() {
    let candidates = vec![
        candidate(1, dec!(1), dec!(0), dec!(1), 0),
        candidate(2, dec!(4), dec!(0), dec!(1), 0),
    ];

    let ladder = build_ladder(&candidates, dec!(100)).expect("ladder");
    assert_eq!(ladder.pool_value_base, Decimal::ZERO);
    assert_eq!(ladder.candidate_count, 2);

    let rung = &ladder.rungs[1];
    assert_eq!(rung.selected_count, 2);
    assert_eq!(rung.effective_trade_count, 2);
    assert_eq!(rung.trades.len(), 2);
    assert!(rung.trades.iter().all(|trade| trade.side == TradeSide::Buy));
    assert!(rung.trades.iter().any(|trade| {
        trade.instrument_id == 1 && trade.shares == 20 && trade.amount_base == dec!(20)
    }));
    assert!(rung.trades.iter().any(|trade| {
        trade.instrument_id == 2 && trade.shares == 80 && trade.amount_base == dec!(80)
    }));
    assert_eq!(rung.achieved_net_base, dec!(100));
    assert_eq!(rung.residual_base, Decimal::ZERO);
    assert_eq!(rung.coverage_percent.expect("coverage"), dec!(100));
}

#[test]
fn zero_pool_with_nonpositive_cash_is_unavailable() {
    let candidates = vec![
        candidate(1, dec!(1), dec!(0), dec!(1), 0),
        candidate(2, dec!(4), dec!(0), dec!(1), 0),
    ];

    assert_eq!(
        build_ladder(&candidates, Decimal::ZERO).unwrap_err(),
        RebalanceUnavailable::OffsetExceedsPool
    );
    assert_eq!(
        build_ladder(&candidates, dec!(-1)).unwrap_err(),
        RebalanceUnavailable::OffsetExceedsPool
    );
}

#[test]
fn zero_value_buy_only_candidate_can_survive_residual_repair() {
    let candidates = vec![
        candidate(1, dec!(1), dec!(100), dec!(1), 100),
        candidate(2, dec!(4), dec!(0), dec!(4), 0),
    ];
    let quantities = resolve_quantities(
        &candidates,
        &[false, true],
        &[dec!(-1), dec!(1)],
        &[dec!(0), dec!(1)],
        dec!(3),
        false,
    );

    assert_eq!(quantities.quantities, vec![0, 1]);
    assert_eq!(quantities.iterations, 1);
}

#[test]
fn buy_presence_repairs_a_naive_sell_top_rung() {
    let candidates = vec![
        candidate(1, dec!(1), dec!(300), dec!(1), 300),
        candidate(2, dec!(1), dec!(100), dec!(1), 100),
        candidate(3, dec!(1), dec!(100), dec!(1), 100),
    ];
    let ladder = build_ladder(&candidates, dec!(10)).expect("ladder");
    let rung = &ladder.rungs[0];
    assert_eq!(rung.selected_count, 1);
    assert_eq!(rung.effective_trade_count, 1);
    assert_eq!(rung.trades[0].side, TradeSide::Buy);
    assert_eq!(rung.trades[0].instrument_id, 2);
}

#[test]
fn sell_presence_repairs_a_naive_buy_top_rung() {
    let candidates = vec![
        candidate(1, dec!(1), dec!(100), dec!(1), 100),
        candidate(2, dec!(1), dec!(300), dec!(1), 300),
        candidate(3, dec!(1), dec!(600), dec!(1), 600),
    ];
    let ladder = build_ladder(&candidates, dec!(-10)).expect("ladder");
    let rung = &ladder.rungs[0];
    assert_eq!(rung.selected_count, 1);
    assert_eq!(rung.effective_trade_count, 1);
    assert_eq!(rung.trades[0].side, TradeSide::Sell);
    assert_eq!(rung.trades[0].instrument_id, 3);
}

#[test]
fn sell_capacity_swaps_in_more_sells_when_needed() {
    let candidates = vec![
        candidate(1, dec!(1), dec!(220), dec!(1), 220),
        candidate(2, dec!(1), dec!(30), dec!(1), 30),
        candidate(3, dec!(1), dec!(200), dec!(1), 200),
        candidate(4, dec!(1), dec!(190), dec!(1), 190),
        candidate(5, dec!(1), dec!(180), dec!(1), 180),
        candidate(6, dec!(1), dec!(170), dec!(1), 170),
    ];
    let ladder = build_ladder(&candidates, dec!(-270)).expect("ladder");
    let rung = &ladder.rungs[1];
    assert_eq!(rung.selected_count, 2);
    assert!(rung
        .trades
        .iter()
        .any(|trade| trade.instrument_id == 1 && trade.side == TradeSide::Sell));
    assert!(rung
        .trades
        .iter()
        .any(|trade| trade.instrument_id == 3 && trade.side == TradeSide::Sell));
    assert!(!rung.trades.iter().any(|trade| trade.instrument_id == 2));
}

#[test]
fn zero_offset_two_sidedness_swaps_in_the_opposite_side() {
    let candidates = vec![
        candidate(1, dec!(1), dec!(220), dec!(1), 220),
        candidate(2, dec!(1), dec!(30), dec!(1), 30),
        candidate(3, dec!(1), dec!(200), dec!(1), 200),
        candidate(4, dec!(1), dec!(190), dec!(1), 190),
        candidate(5, dec!(1), dec!(180), dec!(1), 180),
    ];
    let ladder = build_ladder(&candidates, Decimal::ZERO).expect("ladder");
    let rung = &ladder.rungs[1];
    assert_eq!(rung.selected_count, 2);
    assert!(rung.trades.iter().any(|trade| trade.side == TradeSide::Buy));
    assert!(rung
        .trades
        .iter()
        .any(|trade| trade.side == TradeSide::Sell));
}

#[test]
fn clamp_iteration_clamps_sells_and_preserves_a_buy() {
    let candidates = vec![
        candidate(1, dec!(1), dec!(220), dec!(10), 22),
        candidate(2, dec!(1), dec!(30), dec!(10), 3),
        candidate(3, dec!(1), dec!(200), dec!(10), 20),
        candidate(4, dec!(1), dec!(190), dec!(10), 19),
        candidate(5, dec!(1), dec!(180), dec!(10), 18),
    ];
    let offset = dec!(100);
    let (pool_value, _, deltas) = ideal_deltas(&candidates, offset);
    let selected = {
        let ranked = ranked_indices(&deltas);
        let mut selected = vec![false; candidates.len()];
        for idx in ranked.into_iter().take(2) {
            selected[idx] = true;
        }
        selected
    };
    let redistribution = redistribute_selected(&candidates, &selected, &deltas, offset);
    assert!(redistribution.passes <= redistribution.selected_free_count);
    assert!(redistribution
        .x
        .iter()
        .zip(deltas.iter())
        .all(|(x, delta)| {
            delta.is_zero()
                || x.is_zero()
                || (x > &Decimal::ZERO && delta > &Decimal::ZERO)
                || (x < &Decimal::ZERO && delta < &Decimal::ZERO)
        }));
    let quantity_state = resolve_quantities(
        &candidates,
        &selected,
        &deltas,
        &redistribution.x,
        offset,
        false,
    );
    let net: Decimal = quantity_state
        .quantities
        .iter()
        .zip(candidates.iter())
        .map(|(q, candidate)| Decimal::from(*q) * candidate.price_base)
        .sum();
    assert_eq!(net, offset);
    assert_eq!(pool_value, dec!(820));
}

#[test]
fn clamped_sell_reports_clamped_reason_and_exact_offset_when_buy_remains() {
    let candidates = vec![
        candidate(1, dec!(1), dec!(100), dec!(10), 10),
        candidate(2, dec!(1), dec!(110), dec!(10), 11),
        candidate(3, dec!(1), dec!(90), dec!(10), 9),
        candidate(4, dec!(1), dec!(90), dec!(10), 9),
        candidate(5, dec!(1), dec!(90), dec!(10), 9),
    ];
    let ideal_deltas = vec![dec!(-100), dec!(110), dec!(90), dec!(90), dec!(90)];

    let rung = build_rung(&candidates, &ideal_deltas, dec!(280), 2);

    assert_eq!(rung.selected_count, 2);
    assert_eq!(rung.effective_trade_count, 1);
    assert_eq!(
        rung.trades,
        vec![super::PlannedTrade {
            instrument_id: 2,
            side: TradeSide::Buy,
            shares: 28,
            price_base: dec!(10),
            amount_base: dec!(280),
        }]
    );
    assert_eq!(
        rung.untraded,
        vec![super::UntradedCandidate {
            instrument_id: 1,
            reason: UntradedReason::Clamped,
        }]
    );
    assert_eq!(rung.achieved_net_base, dec!(280));
    assert_eq!(rung.residual_base, Decimal::ZERO);
}

#[test]
fn all_zero_fallback_clears_stale_clamped_reason() {
    let candidates = vec![candidate(1, dec!(1), dec!(100), dec!(1000), 1)];
    let ideal_deltas = vec![dec!(-1)];

    let rung = build_rung(&candidates, &ideal_deltas, dec!(1), 1);

    assert_eq!(rung.effective_trade_count, 0);
    assert_eq!(
        rung.untraded,
        vec![super::UntradedCandidate {
            instrument_id: 1,
            reason: UntradedReason::TooSmall,
        }]
    );
}

#[test]
fn sell_quantities_are_capped_at_held_quantity() {
    let candidates = vec![
        candidate(1, dec!(1), dec!(100), dec!(10), 10),
        candidate(2, dec!(1), dec!(1400), dec!(10), 140),
        candidate(3, dec!(1), dec!(50), dec!(10), 5),
        candidate(4, dec!(1), dec!(50), dec!(10), 5),
        candidate(5, dec!(1), dec!(50), dec!(10), 5),
    ];
    let ideal_deltas = vec![dec!(-1000), dec!(1400), dec!(-50), dec!(-50), dec!(-50)];

    let rung = build_rung(&candidates, &ideal_deltas, dec!(250), 2);

    assert_eq!(rung.selected_count, 2);
    assert_eq!(rung.effective_trade_count, 2);
    assert!(rung.trades.iter().any(|trade| {
        trade.instrument_id == 1
            && trade.side == TradeSide::Sell
            && trade.shares == candidates[0].held_quantity
    }));
    assert!(rung.trades.iter().any(|trade| {
        trade.instrument_id == 2 && trade.side == TradeSide::Buy && trade.shares == 35
    }));
    assert_eq!(rung.achieved_net_base, dec!(250));
    assert_eq!(rung.residual_base, Decimal::ZERO);
}

#[test]
fn one_share_too_expensive_reports_too_small() {
    let candidates = vec![candidate(1, dec!(1), dec!(1000), dec!(1000), 1)];
    let ladder = build_ladder(&candidates, dec!(1)).expect("ladder");
    let rung = &ladder.rungs[0];
    assert_eq!(rung.effective_trade_count, 0);
    assert!(rung
        .untraded
        .iter()
        .all(|candidate| candidate.reason == UntradedReason::TooSmall));
}

#[test]
fn n_contract_allows_identical_adjacent_rungs_and_on_target_reasons() {
    let ladder = build_ladder(&worked_fixture(), Decimal::ZERO).expect("ladder");
    let rung_two = &ladder.rungs[1];
    let rung_three = &ladder.rungs[2];
    assert_eq!(rung_two.trades, rung_three.trades);
    assert!(rung_three
        .untraded
        .iter()
        .any(|candidate| candidate.reason == UntradedReason::OnTarget));
}

#[test]
fn worked_fixture_n1_uses_all_zero_fallback_and_keeps_the_residual_visible() {
    let ladder = build_ladder(&worked_fixture(), Decimal::ZERO).expect("ladder");
    let rung = &ladder.rungs[0];

    assert_eq!(
        rung.trades,
        vec![super::PlannedTrade {
            instrument_id: 2,
            side: TradeSide::Sell,
            shares: 100,
            price_base: dec!(1000),
            amount_base: dec!(100000),
        }]
    );
    assert!(rung.untraded.is_empty());
    assert_eq!(rung.achieved_net_base, dec!(-100000));
    assert_eq!(rung.residual_base, dec!(100000));
}

#[test]
fn coverage_matches_the_definition_and_is_none_for_balanced_pools() {
    let ladder = build_ladder(&worked_fixture(), Decimal::ZERO).expect("ladder");
    let coverages: Vec<_> = ladder
        .rungs
        .iter()
        .map(|rung| rung.coverage_percent.expect("coverage"))
        .collect();
    assert_eq!(coverages[0].round_dp(2), dec!(50));
    assert_eq!(coverages[1].round_dp(2), dec!(100));
    assert_eq!(coverages[2].round_dp(2), dec!(100));
    assert!(coverages.windows(2).all(|window| window[1] >= window[0]));

    let balanced = vec![
        candidate(1, dec!(1), dec!(100), dec!(1), 100),
        candidate(2, dec!(1), dec!(100), dec!(1), 100),
    ];
    let ladder = build_ladder(&balanced, Decimal::ZERO).expect("ladder");
    assert!(ladder
        .rungs
        .iter()
        .all(|rung| rung.coverage_percent.is_none()));
}

#[test]
fn percent_ranking_surfaces_a_low_conviction_watchlist_name_at_n1() {
    let candidates = vec![
        candidate(1, dec!(40), dec!(500), dec!(1), 500),
        candidate(2, dec!(40), dec!(1000), dec!(1), 1000),
        candidate(3, dec!(1), dec!(0), dec!(1), 0),
        candidate(4, dec!(19), dec!(1000), dec!(1), 1000),
    ];

    let sek = build_ladder(&candidates, dec!(101)).expect("ladder");
    assert!(sek
        .rungs
        .iter()
        .take(3)
        .all(|rung| { !rung.trades.iter().any(|trade| trade.instrument_id == 3) }));

    let percent = build_percent_ladder(&candidates, dec!(101)).expect("ladder");
    let rung = &percent.rungs[0];
    assert_eq!(rung.selected_count, 1);
    assert!(rung
        .trades
        .iter()
        .any(|trade| trade.instrument_id == 3 && trade.side == TradeSide::Buy));
}

#[test]
fn percent_tie_breaks_by_weight_then_gap_then_index() {
    let candidates = vec![
        candidate(1, dec!(1), dec!(100), dec!(1), 100),
        candidate(2, dec!(1), dec!(50), dec!(1), 50),
        candidate(3, dec!(1), dec!(50), dec!(1), 50),
        candidate(4, dec!(2), dec!(0), dec!(1), 0),
        candidate(5, dec!(1), dec!(0), dec!(1), 0),
        candidate(6, dec!(1), dec!(40), dec!(1), 40),
    ];
    let targets = vec![
        dec!(200),
        dec!(100),
        dec!(100),
        dec!(100),
        dec!(100),
        dec!(80),
    ];
    let deltas = vec![
        dec!(100),
        dec!(50),
        dec!(50),
        dec!(100),
        dec!(100),
        dec!(40),
    ];

    let ranked = super::ranked_indices(RankBy::Percent, &candidates, &targets, &deltas);
    assert_eq!(ranked, vec![3, 4, 0, 1, 2, 5]);
}

#[test]
fn percent_buy_clamp_leaves_residual_visible_and_sek_absorbs_the_full_offset() {
    let candidates = vec![
        candidate(1, dec!(40), dec!(500), dec!(1), 500),
        candidate(2, dec!(40), dec!(1000), dec!(1), 1000),
        candidate(3, dec!(1), dec!(0), dec!(1), 0),
        candidate(4, dec!(19), dec!(1000), dec!(1), 1000),
    ];

    let percent = build_percent_ladder(&candidates, dec!(101)).expect("ladder");
    let rung = &percent.rungs[0];
    assert_eq!(rung.selected_count, 1);
    assert_eq!(rung.effective_trade_count, 1);
    assert_eq!(rung.trades[0].instrument_id, 3);
    assert_eq!(rung.trades[0].side, TradeSide::Buy);
    assert!(rung.achieved_net_base < dec!(101));
    assert!(rung.residual_base > Decimal::ZERO);

    let sek = build_ladder(&candidates, dec!(101)).expect("ladder");
    let sek_rung = &sek.rungs[0];
    assert_eq!(sek_rung.achieved_net_base, dec!(101));
    assert_eq!(sek_rung.residual_base, Decimal::ZERO);
}

#[test]
fn percent_sell_clamp_stops_at_target_instead_of_zero_value() {
    let candidates = vec![candidate(1, dec!(1), dec!(100), dec!(1), 100)];
    let ideal_deltas = vec![dec!(-60)];

    let rung = build_percent_rung(&candidates, &ideal_deltas, dec!(-80), 1);

    assert_eq!(rung.selected_count, 1);
    assert_eq!(rung.effective_trade_count, 1);
    assert_eq!(
        rung.trades,
        vec![super::PlannedTrade {
            instrument_id: 1,
            side: TradeSide::Sell,
            shares: 60,
            price_base: dec!(1),
            amount_base: dec!(60),
        }]
    );
    assert_eq!(rung.balance[0].gap_after_base, Decimal::ZERO);
    assert_eq!(rung.balance[0].status_after, TargetStatus::OnTarget);
}

#[test]
fn percent_order_repairs_pick_a_different_candidate_than_sek() {
    let candidates = vec![
        candidate(1, dec!(1), dec!(250), dec!(1), 250),
        candidate(2, dec!(1), dec!(300), dec!(1), 300),
        candidate(3, dec!(1), dec!(50), dec!(1), 50),
    ];
    let ideal_deltas = vec![dec!(-150), dec!(100), dec!(50)];

    let sek = build_rung(&candidates, &ideal_deltas, dec!(50), 1);
    let percent = build_percent_rung(&candidates, &ideal_deltas, dec!(50), 1);

    assert_eq!(sek.trades[0].instrument_id, 2);
    assert_eq!(percent.trades[0].instrument_id, 3);
}

#[test]
fn percent_sell_capacity_continues_swapping_until_target_sell_capacity_is_met() {
    let candidates = vec![
        candidate(1, dec!(1), dec!(100), dec!(1), 100),
        candidate(2, dec!(1), dec!(1000), dec!(1), 1000),
        candidate(3, dec!(1), dec!(800), dec!(1), 800),
        candidate(4, dec!(1), dec!(850), dec!(1), 850),
    ];
    let ideal_deltas = vec![dec!(500), dec!(-10), dec!(-200), dec!(-150)];

    let sek = build_rung(&candidates, &ideal_deltas, dec!(-300), 2);
    let percent = build_percent_rung(&candidates, &ideal_deltas, dec!(-300), 2);

    assert!(sek.trades.iter().any(|trade| trade.instrument_id == 1));
    assert!(sek.trades.iter().any(|trade| trade.instrument_id == 3));
    assert!(percent.trades.iter().any(|trade| trade.instrument_id == 3));
    assert!(percent.trades.iter().any(|trade| trade.instrument_id == 4));
    assert!(!percent.trades.iter().any(|trade| trade.instrument_id == 1));
}

#[test]
fn percent_greedy_pass_never_crosses_the_target() {
    let candidates = vec![candidate(1, dec!(1), dec!(100), dec!(10), 10)];
    let selected = vec![true];
    let ideal_deltas = vec![dec!(15)];

    let sek = resolve_quantities(
        &candidates,
        &selected,
        &ideal_deltas,
        &[dec!(14)],
        dec!(20),
        false,
    );
    let percent = resolve_quantities_percent(
        &candidates,
        &selected,
        &ideal_deltas,
        &[dec!(14)],
        dec!(20),
        false,
    );

    assert_eq!(sek.quantities, vec![2]);
    assert_eq!(sek.iterations, 1);
    assert_eq!(percent.quantities, vec![1]);
    assert_eq!(percent.iterations, 0);
}

#[test]
fn percent_rounds_toward_zero_at_the_boundary() {
    let candidates = vec![candidate(1, dec!(1), dec!(100), dec!(10), 10)];
    let selected = vec![true];
    let ideal_deltas = vec![dec!(15)];

    let sek = resolve_quantities(
        &candidates,
        &selected,
        &ideal_deltas,
        &[dec!(15)],
        dec!(15),
        true,
    );
    let percent = resolve_quantities_percent(
        &candidates,
        &selected,
        &ideal_deltas,
        &[dec!(15)],
        dec!(15),
        true,
    );

    assert_eq!(sek.quantities, vec![2]);
    assert_eq!(percent.quantities, vec![1]);
}

#[test]
fn percent_coverage_stays_within_bounds_at_every_rung() {
    let candidates = vec![
        candidate(1, dec!(40), dec!(500), dec!(1), 500),
        candidate(2, dec!(40), dec!(1000), dec!(1), 1000),
        candidate(3, dec!(1), dec!(0), dec!(1), 0),
        candidate(4, dec!(19), dec!(1000), dec!(1), 1000),
    ];
    let ladder = build_percent_ladder(&candidates, dec!(101)).expect("ladder");

    for rung in &ladder.rungs {
        let coverage = rung.coverage_percent.expect("coverage");
        assert!(coverage >= Decimal::ZERO);
        assert!(coverage <= dec!(100));
    }
}

#[test]
fn percent_mode_is_deterministic_for_identical_inputs() {
    let candidates = vec![
        candidate(1, dec!(40), dec!(500), dec!(1), 500),
        candidate(2, dec!(40), dec!(1000), dec!(1), 1000),
        candidate(3, dec!(1), dec!(0), dec!(1), 0),
        candidate(4, dec!(19), dec!(1000), dec!(1), 1000),
    ];

    let left = build_percent_ladder(&candidates, dec!(101)).expect("ladder");
    let right = build_percent_ladder(&candidates, dec!(101)).expect("ladder");

    assert_eq!(left, right);
}

#[test]
fn balance_reports_before_after_gaps_and_side_flips() {
    // Equal weights, pool 300, target 100 each: gaps +40, −25, −15.
    let candidates = vec![
        candidate(1, dec!(1), dec!(140), dec!(0.5), 280),
        candidate(2, dec!(1), dec!(75), dec!(0.5), 150),
        candidate(3, dec!(1), dec!(85), dec!(0.5), 170),
    ];
    let ladder = build_ladder(&candidates, Decimal::ZERO).expect("ladder");

    // N = 2 selects instruments 1 and 2 (largest |delta|). The net-zero
    // constraint spreads the unselected −15 across them: sell 32.50 of 1,
    // buy 32.50 of 2 — pushing instrument 2 past its target.
    let rung = &ladder.rungs[1];
    let balance = &rung.balance;

    assert_eq!(balance[0].gap_before_base, dec!(40));
    assert_eq!(balance[1].gap_before_base, dec!(-25));
    assert_eq!(balance[2].gap_before_base, dec!(-15));
    assert_eq!(balance[0].status_before, TargetStatus::Above);
    assert_eq!(balance[1].status_before, TargetStatus::Below);

    assert_eq!(balance[0].gap_after_base, dec!(7.5));
    assert_eq!(balance[1].gap_after_base, dec!(7.5));
    assert_eq!(balance[1].status_after, TargetStatus::Above); // flipped side
                                                              // Unselected candidate is untouched.
    assert_eq!(balance[2].gap_after_base, balance[2].gap_before_base);

    assert_eq!(balance[1].gap_before_percent, Some(dec!(-25)));
    assert_eq!(rung.total_gap_before_base, dec!(80));
    assert_eq!(rung.total_gap_after_base, dec!(30));
}

#[test]
fn balance_gap_after_sums_to_minus_residual_at_every_rung() {
    let ladder = build_ladder(&worked_fixture(), dec!(50000)).expect("ladder");
    for rung in &ladder.rungs {
        let sum_after: Decimal = rung.balance.iter().map(|b| b.gap_after_base).sum();
        assert_eq!(sum_after, -rung.residual_base);
        let total_before: Decimal = rung.balance.iter().map(|b| b.gap_before_base.abs()).sum();
        let total_after: Decimal = rung.balance.iter().map(|b| b.gap_after_base.abs()).sum();
        assert_eq!(total_before, rung.total_gap_before_base);
        assert_eq!(total_after, rung.total_gap_after_base);
    }
}

#[test]
fn effective_trade_count_never_exceeds_selected_count() {
    let ladder = build_ladder(&worked_fixture(), Decimal::ZERO).expect("ladder");
    assert!(ladder
        .rungs
        .iter()
        .all(|rung| rung.effective_trade_count <= rung.selected_count));
}

#[test]
fn midpoint_rounding_is_away_from_zero() {
    let candidates = vec![candidate(1, dec!(1), dec!(10), dec!(10), 1)];
    let selected = vec![true];
    let ideal_deltas = vec![dec!(10)];

    let quantities = resolve_quantities(
        &candidates,
        &selected,
        &ideal_deltas,
        &[dec!(15)],
        dec!(15),
        false,
    );

    assert_eq!(quantities.quantities, vec![2]);
    assert_eq!(quantities.iterations, 0);
}

#[test]
fn residual_pass_can_revive_a_zero_buy_only_within_constraints() {
    let candidates = vec![candidate(1, dec!(1), dec!(10), dec!(5), 2)];
    let selected = vec![true];
    let ideal_deltas = vec![dec!(3)];

    let quantities = resolve_quantities(
        &candidates,
        &selected,
        &ideal_deltas,
        &[dec!(2)],
        dec!(3),
        false,
    );

    assert_eq!(quantities.quantities, vec![1]);
    assert_eq!(quantities.iterations, 1);
}

#[test]
fn residual_matches_requested_offset_at_every_rung() {
    let ladder = build_ladder(&worked_fixture(), dec!(-25000)).expect("ladder");
    for rung in &ladder.rungs {
        assert_eq!(rung.residual_base, dec!(-25000) - rung.achieved_net_base);
    }
}

#[test]
fn unavailable_states_are_explicit() {
    assert_eq!(
        build_ladder(&[], Decimal::ZERO).unwrap_err(),
        RebalanceUnavailable::EmptyPool
    );
    let ladder = worked_fixture();
    let pool_value: Decimal = ladder
        .iter()
        .map(|candidate| candidate.market_value_base)
        .sum();
    assert_eq!(
        build_ladder(&ladder, -pool_value).unwrap_err(),
        RebalanceUnavailable::OffsetExceedsPool
    );
    assert_eq!(
        build_ladder(&ladder, -pool_value - Decimal::ONE).unwrap_err(),
        RebalanceUnavailable::OffsetExceedsPool
    );
}
