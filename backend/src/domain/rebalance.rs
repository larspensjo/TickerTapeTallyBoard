//! Pure rebalance ladder construction.
//!
//! This module is intentionally data-only: callers provide already-filtered
//! candidates and a base-currency offset, and the ladder returns whole-share
//! trade previews plus explicit untraded reasons.

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::{Decimal, RoundingStrategy};
use std::cmp::Ordering;

use super::conviction::{gap_band_status, TargetStatus};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RankBy {
    Sek,
    Percent,
}

#[derive(Clone, Debug, PartialEq)]
struct IdealAllocation {
    pool_value_base: Decimal,
    targets: Vec<Decimal>,
    deltas: Vec<Decimal>,
}

#[derive(Clone, Copy, Debug)]
struct RungContext<'a> {
    candidates: &'a [RebalanceCandidate],
    deltas: &'a [Decimal],
    targets: &'a [Decimal],
    offset: Decimal,
    rank_by: RankBy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TradeSide {
    Buy,
    Sell,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RebalanceCandidate {
    pub instrument_id: i64,
    pub weight: Decimal,
    /// Current market value in base currency. May be zero for watchlist rows.
    pub market_value_base: Decimal,
    /// Latest tradeable price in base currency per share. Must stay positive.
    pub price_base: Decimal,
    /// Quantity currently held. May be zero for buy-only watchlist rows.
    pub held_quantity: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlannedTrade {
    pub instrument_id: i64,
    pub side: TradeSide,
    pub shares: i64,
    pub price_base: Decimal,
    pub amount_base: Decimal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UntradedReason {
    TooSmall,
    Clamped,
    OnTarget,
}

impl UntradedReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TooSmall => "too_small",
            Self::Clamped => "clamped",
            Self::OnTarget => "on_target",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UntradedCandidate {
    pub instrument_id: i64,
    pub reason: UntradedReason,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CandidateBalance {
    pub instrument_id: i64,
    /// Signed display gap vs the post-offset target: value − target.
    /// Positive = above target. Note: equals −ideal_delta.
    pub gap_before_base: Decimal,
    /// gap_before_base + this rung's net traded amount for the candidate.
    pub gap_after_base: Decimal,
    /// Gaps as percent of the post-offset target; None when the target is
    /// not strictly positive (defensive; not expected in practice).
    pub gap_before_percent: Option<Decimal>,
    pub gap_after_percent: Option<Decimal>,
    pub status_before: TargetStatus,
    pub status_after: TargetStatus,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RebalanceRung {
    pub selected_count: usize,
    pub trades: Vec<PlannedTrade>,
    pub untraded: Vec<UntradedCandidate>,
    pub balance: Vec<CandidateBalance>,
    pub effective_trade_count: usize,
    pub achieved_net_base: Decimal,
    pub residual_base: Decimal,
    pub coverage_percent: Option<Decimal>,
    /// Σ|gap_before| over all candidates (the planner's G).
    pub total_gap_before_base: Decimal,
    /// Σ|gap_after| over all candidates (the planner's G′).
    pub total_gap_after_base: Decimal,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RebalanceLadder {
    pub pool_value_base: Decimal,
    pub candidate_count: usize,
    pub rungs: Vec<RebalanceRung>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RebalanceUnavailable {
    EmptyPool,
    OffsetExceedsPool,
}

impl RebalanceUnavailable {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EmptyPool => "empty_pool",
            Self::OffsetExceedsPool => "offset_exceeds_pool",
        }
    }
}

#[derive(Clone, Debug)]
struct RedistributionState {
    x: Vec<Decimal>,
    clamped_to_zero: Vec<bool>,
    passes: usize,
    selected_free_count: usize,
    all_zero_fallback: bool,
}

#[derive(Clone, Debug)]
struct QuantityState {
    quantities: Vec<i64>,
    iterations: usize,
}

pub fn build_ladder(
    candidates: &[RebalanceCandidate],
    offset: Decimal,
    rank_by: RankBy,
) -> Result<RebalanceLadder, RebalanceUnavailable> {
    if candidates.is_empty() {
        return Err(RebalanceUnavailable::EmptyPool);
    }

    let allocation = ideal_deltas(candidates, offset);
    let context = RungContext {
        candidates,
        deltas: &allocation.deltas,
        targets: &allocation.targets,
        offset,
        rank_by,
    };
    let pool_value_base = allocation.pool_value_base;
    if offset <= -pool_value_base {
        return Err(RebalanceUnavailable::OffsetExceedsPool);
    }

    let mut rungs = Vec::with_capacity(candidates.len());
    for selected_count in 1..=candidates.len() {
        rungs.push(build_rung(&context, selected_count));
    }

    Ok(RebalanceLadder {
        pool_value_base,
        candidate_count: candidates.len(),
        rungs,
    })
}

fn ideal_deltas(candidates: &[RebalanceCandidate], offset: Decimal) -> IdealAllocation {
    let mut pool_value_base = Decimal::ZERO;
    let mut total_weight = Decimal::ZERO;
    for candidate in candidates {
        pool_value_base += candidate.market_value_base;
        total_weight += candidate.weight;
    }
    debug_assert!(pool_value_base >= Decimal::ZERO);
    debug_assert!(total_weight > Decimal::ZERO);

    let pool_plus_offset = pool_value_base + offset;
    let mut targets = Vec::with_capacity(candidates.len());
    let mut deltas = Vec::with_capacity(candidates.len());
    let mut allocated_target = Decimal::ZERO;

    for (idx, candidate) in candidates.iter().enumerate() {
        let target = if idx + 1 == candidates.len() {
            // The final target absorbs Decimal division remainder so deltas
            // sum exactly to the requested offset.
            pool_plus_offset - allocated_target
        } else {
            let target = pool_plus_offset * candidate.weight / total_weight;
            allocated_target += target;
            target
        };
        targets.push(target);
        deltas.push(target - candidate.market_value_base);
    }

    IdealAllocation {
        pool_value_base,
        targets,
        deltas,
    }
}

fn ranked_indices(
    rank_by: RankBy,
    candidates: &[RebalanceCandidate],
    targets: &[Decimal],
    ideal_deltas: &[Decimal],
) -> Vec<usize> {
    let mut ranked: Vec<usize> = (0..ideal_deltas.len()).collect();
    ranked.sort_by(|left, right| match rank_by {
        RankBy::Sek => {
            let left_abs = ideal_deltas[*left].abs();
            let right_abs = ideal_deltas[*right].abs();
            right_abs.cmp(&left_abs).then_with(|| left.cmp(right))
        }
        RankBy::Percent => {
            let left_target = targets[*left];
            let right_target = targets[*right];
            let left_positive = left_target > Decimal::ZERO;
            let right_positive = right_target > Decimal::ZERO;

            match (left_positive, right_positive) {
                (true, true) => {
                    let left_abs = ideal_deltas[*left].abs();
                    let right_abs = ideal_deltas[*right].abs();
                    let left_key = left_abs * right_target;
                    let right_key = right_abs * left_target;
                    right_key
                        .cmp(&left_key)
                        .then_with(|| candidates[*right].weight.cmp(&candidates[*left].weight))
                        .then_with(|| right_abs.cmp(&left_abs))
                        .then_with(|| left.cmp(right))
                }
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                (false, false) => candidates[*right]
                    .weight
                    .cmp(&candidates[*left].weight)
                    .then_with(|| ideal_deltas[*right].abs().cmp(&ideal_deltas[*left].abs()))
                    .then_with(|| left.cmp(right)),
            }
        }
    });
    ranked
}

fn highest_ranked_nonselected(
    ranked: &[usize],
    selected: &[bool],
    ideal_deltas: &[Decimal],
    predicate: impl Fn(Decimal) -> bool,
) -> Option<usize> {
    ranked
        .iter()
        .copied()
        .find(|idx| !selected[*idx] && predicate(ideal_deltas[*idx]))
}

fn lowest_ranked_selected(
    ranked: &[usize],
    selected: &[bool],
    ideal_deltas: &[Decimal],
    predicate: impl Fn(Decimal) -> bool,
) -> Option<usize> {
    ranked
        .iter()
        .rev()
        .copied()
        .find(|idx| selected[*idx] && predicate(ideal_deltas[*idx]))
}

fn select_indices_for_rung(context: &RungContext<'_>, selected_count: usize) -> Vec<bool> {
    let ranked = ranked_indices(
        context.rank_by,
        context.candidates,
        context.targets,
        context.deltas,
    );
    let mut selected = vec![false; context.candidates.len()];
    for idx in ranked.iter().take(selected_count).copied() {
        selected[idx] = true;
    }

    if context.offset > Decimal::ZERO
        && !selected
            .iter()
            .enumerate()
            .any(|(idx, is_selected)| *is_selected && context.deltas[idx] > Decimal::ZERO)
    {
        let Some(from_idx) = lowest_ranked_selected(&ranked, &selected, context.deltas, |_| true)
        else {
            return selected;
        };
        let Some(to_idx) =
            highest_ranked_nonselected(&ranked, &selected, context.deltas, |delta| {
                delta > Decimal::ZERO
            })
        else {
            return selected;
        };
        selected[from_idx] = false;
        selected[to_idx] = true;
    }

    if context.offset < Decimal::ZERO {
        if !selected
            .iter()
            .enumerate()
            .any(|(idx, is_selected)| *is_selected && context.deltas[idx] < Decimal::ZERO)
        {
            let Some(from_idx) =
                lowest_ranked_selected(&ranked, &selected, context.deltas, |_| true)
            else {
                return selected;
            };
            let Some(to_idx) =
                highest_ranked_nonselected(&ranked, &selected, context.deltas, |delta| {
                    delta < Decimal::ZERO
                })
            else {
                return selected;
            };
            selected[from_idx] = false;
            selected[to_idx] = true;
        }

        while selected
            .iter()
            .enumerate()
            .filter(|(idx, is_selected)| **is_selected && context.deltas[*idx] < Decimal::ZERO)
            .map(|(idx, _)| match context.rank_by {
                RankBy::Sek => context.candidates[idx].market_value_base,
                RankBy::Percent => -context.deltas[idx],
            })
            .sum::<Decimal>()
            < -context.offset
            && selected
                .iter()
                .enumerate()
                .any(|(idx, is_selected)| *is_selected && context.deltas[idx] >= Decimal::ZERO)
            && ranked
                .iter()
                .any(|idx| !selected[*idx] && context.deltas[*idx] < Decimal::ZERO)
        {
            let Some(from_idx) =
                lowest_ranked_selected(&ranked, &selected, context.deltas, |delta| {
                    delta >= Decimal::ZERO
                })
            else {
                break;
            };
            let Some(to_idx) =
                highest_ranked_nonselected(&ranked, &selected, context.deltas, |delta| {
                    delta < Decimal::ZERO
                })
            else {
                break;
            };
            selected[from_idx] = false;
            selected[to_idx] = true;
        }
    }

    if context.offset.is_zero() && selected_count >= 2 {
        let mut nonzero_sign: Option<bool> = None;
        let mut same_sign = true;
        for (idx, is_selected) in selected.iter().enumerate() {
            if !*is_selected || context.deltas[idx].is_zero() {
                continue;
            }
            let sign = context.deltas[idx] > Decimal::ZERO;
            match nonzero_sign {
                Some(previous) if previous != sign => {
                    same_sign = false;
                    break;
                }
                Some(_) => {}
                None => nonzero_sign = Some(sign),
            }
        }

        if same_sign {
            let Some(sign_is_positive) = nonzero_sign else {
                return selected;
            };
            let Some(from_idx) =
                lowest_ranked_selected(&ranked, &selected, context.deltas, |_| true)
            else {
                return selected;
            };
            let Some(to_idx) =
                highest_ranked_nonselected(&ranked, &selected, context.deltas, |delta| {
                    if sign_is_positive {
                        delta < Decimal::ZERO
                    } else {
                        delta > Decimal::ZERO
                    }
                })
            else {
                return selected;
            };
            selected[from_idx] = false;
            selected[to_idx] = true;
        }
    }

    selected
}

fn redistribute_selected(context: &RungContext<'_>, selected: &[bool]) -> RedistributionState {
    let mut x = vec![Decimal::ZERO; context.candidates.len()];
    let mut fixed = vec![false; context.candidates.len()];
    let mut clamped_to_zero = vec![false; context.candidates.len()];
    let mut free = vec![false; context.candidates.len()];
    let mut all_zero_fallback = false;

    for idx in 0..context.candidates.len() {
        if !selected[idx] {
            continue;
        }
        if context.deltas[idx].is_zero() {
            fixed[idx] = true;
        } else {
            free[idx] = true;
        }
    }

    let selected_free_count = free.iter().filter(|is_free| **is_free).count();
    let mut passes = 0usize;

    loop {
        if !free.iter().any(|is_free| *is_free) {
            break;
        }

        let sum_fixed = (0..context.candidates.len())
            .filter(|idx| fixed[*idx])
            .map(|idx| x[idx])
            .sum::<Decimal>();
        let sum_free_d = (0..context.candidates.len())
            .filter(|idx| free[*idx])
            .map(|idx| context.deltas[idx])
            .sum::<Decimal>();
        let sum_free_w = (0..context.candidates.len())
            .filter(|idx| free[*idx])
            .map(|idx| context.candidates[idx].weight)
            .sum::<Decimal>();
        debug_assert!(sum_free_w > Decimal::ZERO);

        let residual = context.offset - sum_fixed - sum_free_d;
        for idx in 0..context.candidates.len() {
            if free[idx] {
                x[idx] =
                    context.deltas[idx] + residual * context.candidates[idx].weight / sum_free_w;
            }
        }

        let mut violators = Vec::new();
        for idx in 0..context.candidates.len() {
            if !free[idx] {
                continue;
            }

            let delta = context.deltas[idx];
            let value = context.candidates[idx].market_value_base;
            if (delta > Decimal::ZERO && x[idx] < Decimal::ZERO)
                || (delta < Decimal::ZERO && x[idx] > Decimal::ZERO)
            {
                x[idx] = Decimal::ZERO;
                clamped_to_zero[idx] = true;
                violators.push(idx);
            } else if context.rank_by == RankBy::Sek && delta < Decimal::ZERO && x[idx] < -value {
                x[idx] = -value;
                violators.push(idx);
            } else if context.rank_by == RankBy::Percent
                && ((delta > Decimal::ZERO && x[idx] > delta)
                    || (delta < Decimal::ZERO && x[idx] < delta))
            {
                x[idx] = delta;
                violators.push(idx);
            }
        }

        if violators.is_empty() {
            break;
        }

        for idx in violators {
            free[idx] = false;
            fixed[idx] = true;
        }
        passes += 1;
        debug_assert!(passes <= selected_free_count);
    }

    let selected_has_nonzero = selected
        .iter()
        .enumerate()
        .any(|(idx, is_selected)| *is_selected && !context.deltas[idx].is_zero());
    if selected_has_nonzero
        && selected
            .iter()
            .enumerate()
            .all(|(idx, is_selected)| !*is_selected || x[idx].is_zero())
    {
        all_zero_fallback = true;
        for idx in 0..context.candidates.len() {
            if selected[idx] && !context.deltas[idx].is_zero() {
                clamped_to_zero[idx] = false;
                x[idx] = context.deltas[idx];
            }
        }
    }

    RedistributionState {
        x,
        clamped_to_zero,
        passes,
        selected_free_count,
        all_zero_fallback,
    }
}

fn resolve_quantities(
    context: &RungContext<'_>,
    selected: &[bool],
    x: &[Decimal],
    skip_greedy_pass: bool,
) -> QuantityState {
    let mut quantities = vec![0i64; context.candidates.len()];
    let mut min_price = None::<Decimal>;

    for idx in 0..context.candidates.len() {
        if !selected[idx] {
            continue;
        }

        let candidate = &context.candidates[idx];
        min_price = Some(match min_price {
            Some(current) => current.min(candidate.price_base),
            None => candidate.price_base,
        });

        let rounded = (x[idx] / candidate.price_base).round_dp_with_strategy(
            0,
            if context.rank_by == RankBy::Percent {
                RoundingStrategy::ToZero
            } else {
                RoundingStrategy::MidpointAwayFromZero
            },
        );
        let mut quantity = rounded
            .to_i64()
            .expect("rounded whole-share quantity fits in i64");

        if (context.deltas[idx] > Decimal::ZERO && quantity < 0)
            || (context.deltas[idx] < Decimal::ZERO && quantity > 0)
        {
            quantity = 0;
        }

        if context.deltas[idx] < Decimal::ZERO && quantity < -candidate.held_quantity {
            quantity = -candidate.held_quantity;
        }

        quantities[idx] = quantity;
    }

    let mut residual = context.offset
        - (0..context.candidates.len())
            .map(|idx| Decimal::from(quantities[idx]) * context.candidates[idx].price_base)
            .sum::<Decimal>();
    let initial_residual = residual;
    let p_min = min_price.expect("at least one selected candidate");
    let iteration_cap = if initial_residual.is_zero() {
        0usize
    } else {
        let quotient = (initial_residual.abs() / p_min)
            .round_dp_with_strategy(0, RoundingStrategy::AwayFromZero);
        quotient.to_u64().expect("iteration cap fits in u64") as usize + context.candidates.len()
    };

    if skip_greedy_pass {
        return QuantityState {
            quantities,
            iterations: 0,
        };
    }

    let mut iterations = 0usize;
    while !residual.is_zero() {
        let mut best: Option<(usize, i64)> = None;
        for idx in 0..context.candidates.len() {
            if !selected[idx] {
                continue;
            }

            let candidate = &context.candidates[idx];
            let delta_q = if residual > Decimal::ZERO {
                if context.deltas[idx] > Decimal::ZERO {
                    if context.rank_by == RankBy::Percent
                        && Decimal::from(quantities[idx] + 1) * candidate.price_base
                            > context.deltas[idx]
                    {
                        None
                    } else {
                        Some(1)
                    }
                } else if context.deltas[idx] < Decimal::ZERO && quantities[idx] < 0 {
                    Some(1)
                } else {
                    None
                }
            } else if context.deltas[idx] > Decimal::ZERO {
                if quantities[idx] > 0 {
                    Some(-1)
                } else {
                    None
                }
            } else if context.deltas[idx] < Decimal::ZERO {
                if quantities[idx] > -candidate.held_quantity {
                    if context.rank_by == RankBy::Percent
                        && Decimal::from(quantities[idx] - 1) * candidate.price_base
                            < context.deltas[idx]
                    {
                        None
                    } else {
                        Some(-1)
                    }
                } else {
                    None
                }
            } else {
                None
            };

            let Some(delta_q) = delta_q else {
                continue;
            };
            if candidate.price_base >= Decimal::from(2) * residual.abs() {
                continue;
            }

            match best {
                None => best = Some((idx, delta_q)),
                Some((best_idx, _)) => {
                    let best_price = context.candidates[best_idx].price_base;
                    if candidate.price_base < best_price
                        || (candidate.price_base == best_price && idx < best_idx)
                    {
                        best = Some((idx, delta_q));
                    }
                }
            }
        }

        let Some((idx, delta_q)) = best else {
            break;
        };

        quantities[idx] += delta_q;
        residual -= Decimal::from(delta_q) * context.candidates[idx].price_base;
        iterations += 1;
        debug_assert!(iterations <= iteration_cap);
    }

    QuantityState {
        quantities,
        iterations,
    }
}

fn build_rung(context: &RungContext<'_>, selected_count: usize) -> RebalanceRung {
    let selected = select_indices_for_rung(context, selected_count);
    let redistribution = redistribute_selected(context, &selected);
    let quantities = resolve_quantities(
        context,
        &selected,
        &redistribution.x,
        redistribution.all_zero_fallback,
    );

    let mut trades = Vec::new();
    let mut untraded = Vec::new();
    let mut balance = Vec::with_capacity(context.candidates.len());
    let mut achieved_net_base = Decimal::ZERO;
    let mut g = Decimal::ZERO;
    let mut g_prime = Decimal::ZERO;

    for (idx, candidate) in context.candidates.iter().enumerate() {
        let ideal_delta = context.deltas[idx];
        let actual_net = Decimal::from(quantities.quantities[idx]) * candidate.price_base;
        let gap_before = -ideal_delta;
        let gap_after = gap_before + actual_net;
        let target = context.targets[idx];
        let (gap_before_percent, gap_after_percent, status_before, status_after) =
            if target > Decimal::ZERO {
                let hundred = Decimal::from(100);
                let before_percent = gap_before / target * hundred;
                let after_percent = gap_after / target * hundred;
                (
                    Some(before_percent),
                    Some(after_percent),
                    gap_band_status(before_percent),
                    gap_band_status(after_percent),
                )
            } else {
                (
                    None,
                    None,
                    TargetStatus::Unavailable,
                    TargetStatus::Unavailable,
                )
            };

        balance.push(CandidateBalance {
            instrument_id: candidate.instrument_id,
            gap_before_base: gap_before,
            gap_after_base: gap_after,
            gap_before_percent,
            gap_after_percent,
            status_before,
            status_after,
        });

        achieved_net_base += actual_net;
        g += ideal_delta.abs();
        g_prime += (actual_net - ideal_delta).abs();

        if quantities.quantities[idx] != 0 {
            let shares = quantities.quantities[idx].abs();
            let side = if quantities.quantities[idx] > 0 {
                TradeSide::Buy
            } else {
                TradeSide::Sell
            };
            trades.push(PlannedTrade {
                instrument_id: candidate.instrument_id,
                side,
                shares,
                price_base: candidate.price_base,
                amount_base: Decimal::from(shares) * candidate.price_base,
            });
        } else if selected[idx] {
            let reason = if context.deltas[idx].is_zero() {
                UntradedReason::OnTarget
            } else if redistribution.clamped_to_zero[idx] {
                UntradedReason::Clamped
            } else {
                UntradedReason::TooSmall
            };
            untraded.push(UntradedCandidate {
                instrument_id: candidate.instrument_id,
                reason,
            });
        }
    }

    let coverage_percent = if g.is_zero() {
        None
    } else {
        Some(((g - g_prime) / g) * Decimal::from(100))
    };

    let effective_trade_count = trades.len();

    RebalanceRung {
        selected_count,
        trades,
        untraded,
        balance,
        effective_trade_count,
        achieved_net_base,
        residual_base: context.offset - achieved_net_base,
        coverage_percent,
        total_gap_before_base: g,
        total_gap_after_base: g_prime,
    }
}

#[cfg(test)]
mod tests {
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
}
