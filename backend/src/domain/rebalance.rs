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
mod tests;
