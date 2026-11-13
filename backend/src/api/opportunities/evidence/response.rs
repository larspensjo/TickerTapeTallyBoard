//! Stable JSON contract; all percentage values are formatted at this boundary.
#[cfg(test)]
mod tests;

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Serialize;

use crate::{
    api::{fundamentals::ReportDateResponse, valuation::money_string},
    domain::{
        company_figures::{EpsConsistency, EpsConsistencyState, Reason},
        opportunity_evidence::{
            business_check::{BusinessOutcome, BusinessState, BusinessSubtest},
            dividend_check::DividendOutcome,
            market_reference::{MarketOutcome, MarketReferenceSource, MarketState},
            price_checks::{
                OffHighOutcome, OffHighState, StoppedFallingOutcome, UnusualDropOutcome,
            },
            report_timing::{
                LastReportEvidence, ReportComingSoonOutcome, ReportComingSoonState,
                ReportInPeriodOutcome,
            },
            room_to_buy::RoomToBuy,
            verdict::{Caution, DecidingCheck, Verdict},
            EvidenceReason, YesNoUnknown,
        },
        ConvictionTargetOutput, TargetField,
    },
};

#[derive(Debug, Serialize)]
pub struct EvidenceResponse {
    pub verdict: VerdictResponse,
    pub checks: ChecksResponse,
}

#[derive(Debug, Serialize)]
pub struct VerdictResponse {
    pub code: &'static str,
    pub deciding: Vec<&'static str>,
    pub missing: Vec<&'static str>,
    pub cautions: Vec<&'static str>,
}

impl From<Verdict> for VerdictResponse {
    fn from(value: Verdict) -> Self {
        let check = |check| match check {
            DecidingCheck::UnusualDrop => "unusual_drop",
            DecidingCheck::OffHigh => "off_high",
            DecidingCheck::ReportInPeriod => "report_in_period",
            DecidingCheck::Business => "business",
        };
        Self {
            code: value.code.as_str(),
            deciding: value.deciding.into_iter().map(check).collect(),
            missing: value.missing.into_iter().map(check).collect(),
            // Report timing and certainty are available on the named check.
            cautions: value
                .cautions
                .into_iter()
                .map(|c| match c {
                    Caution::LossMaking => "loss_making",
                    Caution::ReportComingSoon(_) => "report_coming_soon",
                    Caution::AlreadyAtTarget => "already_at_target",
                    Caution::NoTarget => "no_target",
                })
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ChecksResponse {
    pub unusual_drop: Check<UnusualDropValues>,
    pub off_high: Check<OffHighValues>,
    pub report_in_period: Check<ReportInPeriodValues>,
    pub report_coming_soon: Check<ReportComingSoonValues>,
    pub business: Check<BusinessValues>,
    pub market: Check<MarketValues>,
    pub stopped_falling: Check<StoppedFallingValues>,
    pub ex_dividend: Check<DividendValues>,
    pub room_to_buy: Check<RoomValues>,
}

#[derive(Debug, Serialize)]
pub struct Check<T> {
    pub state: &'static str,
    pub reasons: Vec<ReasonResponse>,
    #[serde(flatten)]
    pub values: T,
}

#[derive(Debug, Serialize)]
pub struct ReasonResponse {
    pub code: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<NaiveDate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<NaiveDate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<usize>,
}

impl From<Reason> for ReasonResponse {
    fn from(reason: Reason) -> Self {
        let detail = reason.detail.map(|detail| {
            if reason.code == "eps_disagree" {
                detail
                    .parse::<Decimal>()
                    .map(money_string)
                    .unwrap_or(detail)
            } else {
                detail
            }
        });
        Self {
            code: reason.code,
            detail,
            start: None,
            date: None,
            count: None,
            required: None,
        }
    }
}
impl From<EvidenceReason> for ReasonResponse {
    fn from(reason: EvidenceReason) -> Self {
        let mut out = Self::from(Reason::new(reason.as_str()));
        match reason {
            EvidenceReason::MarketIndexUnavailable { display_name } => {
                out.detail = Some(display_name)
            }
            EvidenceReason::PriceHistoryGap { start } => out.start = Some(start),
            EvidenceReason::SplitInLookback { date }
            | EvidenceReason::NonPositiveClose { date } => out.date = Some(date),
            EvidenceReason::PriceHistoryTooShort { closes, required } => {
                out.count = Some(closes);
                out.required = Some(required);
            }
            EvidenceReason::InsufficientMarketRows { count, required } => {
                out.count = Some(count);
                out.required = Some(required);
            }
            EvidenceReason::InsufficientHistoricalMoves { moves } => out.count = Some(moves),
            _ => {}
        }
        out
    }
}

fn yes_no(state: YesNoUnknown) -> &'static str {
    match state {
        YesNoUnknown::Yes => "yes",
        YesNoUnknown::No => "no",
        YesNoUnknown::Unknown => "unknown",
    }
}
fn reasons<R: Into<ReasonResponse>>(values: Vec<R>) -> Vec<ReasonResponse> {
    values.into_iter().map(Into::into).collect()
}
fn percent(value: Option<Decimal>) -> Option<String> {
    value.map(money_string)
}
fn decimal(value: Option<Decimal>) -> Option<String> {
    value.map(|v| v.to_string())
}

#[derive(Debug, Serialize)]
pub struct UnusualDropValues {
    change_percent: String,
    adjusted_change_percent: Option<String>,
    usual_move_percent: Option<String>,
    threshold_percent: Option<String>,
    closes: usize,
    sample_moves: Option<usize>,
}
impl From<UnusualDropOutcome> for Check<UnusualDropValues> {
    fn from(v: UnusualDropOutcome) -> Self {
        // Zero is a real count only when the domain finished building the sample.
        let sample_moves = (v.sample_moves > 0
            || v.reasons
                .iter()
                .any(|r| matches!(r, EvidenceReason::InsufficientHistoricalMoves { .. })))
        .then_some(v.sample_moves);
        Self {
            state: yes_no(v.state),
            reasons: reasons(v.reasons),
            values: UnusualDropValues {
                change_percent: money_string(v.change_percent),
                adjusted_change_percent: percent(v.adjusted_change_percent),
                usual_move_percent: percent(v.usual_move_percent),
                threshold_percent: percent(v.threshold_percent),
                closes: v.closes,
                sample_moves,
            },
        }
    }
}

#[derive(Debug, Serialize)]
pub struct OffHighValues {
    closes: usize,
    latest_close: Option<String>,
    high_close: Option<String>,
    low_close: Option<String>,
    below_high_percent: Option<String>,
    above_low_percent: Option<String>,
    near_low: Option<bool>,
}
impl From<OffHighOutcome> for Check<OffHighValues> {
    fn from(v: OffHighOutcome) -> Self {
        Self {
            state: match v.state {
                OffHighState::Deep => "deep",
                OffHighState::NotDeep => "not_deep",
                OffHighState::Unknown => "unknown",
            },
            reasons: reasons(v.reasons),
            values: OffHighValues {
                closes: v.closes,
                latest_close: decimal(v.latest_close),
                high_close: decimal(v.high_close),
                low_close: decimal(v.low_close),
                below_high_percent: percent(v.below_high_percent),
                above_low_percent: percent(v.above_low_percent),
                near_low: v.near_low,
            },
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LastReportResponse {
    Confirmed {
        date: NaiveDate,
    },
    Interval {
        period_end: NaiveDate,
        absent_on: NaiveDate,
        first_seen: NaiveDate,
    },
    UpperBound {
        period_end: NaiveDate,
        first_seen: NaiveDate,
    },
}
impl From<LastReportEvidence> for LastReportResponse {
    fn from(v: LastReportEvidence) -> Self {
        match v {
            LastReportEvidence::Confirmed { date } => Self::Confirmed { date },
            LastReportEvidence::Interval {
                period_end,
                absent_on,
                first_seen,
            } => Self::Interval {
                period_end,
                absent_on,
                first_seen,
            },
            LastReportEvidence::UpperBound {
                period_end,
                first_seen,
            } => Self::UpperBound {
                period_end,
                first_seen,
            },
        }
    }
}
#[derive(Debug, Serialize)]
pub struct ReportInPeriodValues {
    window_start: NaiveDate,
    window_end: NaiveDate,
    last_report: Option<LastReportResponse>,
    scheduled_report: Option<ReportDateResponse>,
    no_report_expected_before: Option<NaiveDate>,
}
impl From<ReportInPeriodOutcome> for Check<ReportInPeriodValues> {
    fn from(v: ReportInPeriodOutcome) -> Self {
        Self {
            state: v.state.as_str(),
            reasons: reasons(v.reasons),
            values: ReportInPeriodValues {
                window_start: v.window_start,
                window_end: v.window_end,
                last_report: v.last_report.map(Into::into),
                scheduled_report: v.scheduled_report.map(Into::into),
                no_report_expected_before: v.no_report_expected_before,
            },
        }
    }
}
#[derive(Debug, Serialize)]
pub struct ReportComingSoonValues {
    report: ReportDateResponse,
}
impl From<ReportComingSoonOutcome> for Check<ReportComingSoonValues> {
    fn from(v: ReportComingSoonOutcome) -> Self {
        Self {
            state: match v.state {
                ReportComingSoonState::Yes => "yes",
                ReportComingSoonState::No => "no",
                ReportComingSoonState::Unknown => "unknown",
                ReportComingSoonState::NotApplicable => "not_applicable",
            },
            reasons: reasons(v.reasons),
            values: ReportComingSoonValues {
                report: v.report.into(),
            },
        }
    }
}

#[derive(Debug, Serialize)]
pub struct BusinessValues {
    profitable: BusinessSubtestResponse,
    sales_not_shrinking: BusinessSubtestResponse,
    earnings_not_sliding: BusinessSubtestResponse,
    eps_consistency: EpsConsistencyResponse,
}
#[derive(Debug, Serialize)]
pub struct BusinessSubtestResponse {
    passes: Option<bool>,
    value: Option<String>,
    year_ago_value: Option<String>,
    approximate: bool,
    reasons: Vec<ReasonResponse>,
}
impl BusinessSubtestResponse {
    fn new(v: BusinessSubtest, percentage: bool) -> Self {
        let format = |value: Option<Decimal>| {
            if percentage {
                percent(value.map(|v| v * Decimal::ONE_HUNDRED))
            } else {
                decimal(value)
            }
        };
        Self {
            passes: v.passes,
            value: format(v.value),
            year_ago_value: format(v.year_ago_value),
            approximate: v.approximate,
            reasons: reasons(v.reasons),
        }
    }
}
#[derive(Debug, Serialize)]
pub struct EpsConsistencyResponse {
    state: &'static str,
    gap_percent: Option<String>,
    compared_quarters: usize,
}
impl From<EpsConsistency> for EpsConsistencyResponse {
    fn from(v: EpsConsistency) -> Self {
        Self {
            state: match v.state {
                EpsConsistencyState::Unavailable => "unavailable",
                EpsConsistencyState::NotCrossChecked => "not_cross_checked",
                EpsConsistencyState::Consistent => "consistent",
                EpsConsistencyState::Approximate => "approximate",
                EpsConsistencyState::Inconsistent => "inconsistent",
            },
            gap_percent: percent(v.gap_percent),
            compared_quarters: v.compared_quarters,
        }
    }
}
impl From<BusinessOutcome> for Check<BusinessValues> {
    fn from(v: BusinessOutcome) -> Self {
        Self {
            state: match v.state {
                BusinessState::Intact => "intact",
                BusinessState::Weakening => "weakening",
                BusinessState::LossMaking => "loss_making",
                BusinessState::Unknown => "unknown",
                BusinessState::NotApplicable => "not_applicable",
            },
            reasons: reasons(v.reasons),
            values: BusinessValues {
                profitable: BusinessSubtestResponse::new(v.profitable, true),
                sales_not_shrinking: BusinessSubtestResponse::new(v.sales_not_shrinking, true),
                earnings_not_sliding: BusinessSubtestResponse::new(v.earnings_not_sliding, false),
                eps_consistency: v.eps_consistency.into(),
            },
        }
    }
}

#[derive(Debug, Serialize)]
pub struct MarketValues {
    source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    index_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    index_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    index_start_date: Option<NaiveDate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    index_end_date: Option<NaiveDate>,
    change_percent: String,
    reference_percent: Option<String>,
    difference_percent: Option<String>,
    margin_percent: String,
    count: usize,
}
impl From<MarketOutcome> for Check<MarketValues> {
    fn from(v: MarketOutcome) -> Self {
        let (source, index_code, index_name, index_start_date, index_end_date) = match v.source {
            MarketReferenceSource::OtherHoldingsMedian => {
                ("other_holdings_median", None, None, None, None)
            }
            MarketReferenceSource::MarketIndex {
                code,
                display_name,
                start_date,
                end_date,
            } => (
                "market_index",
                Some(code),
                Some(display_name),
                Some(start_date),
                Some(end_date),
            ),
        };
        Self {
            state: match v.state {
                MarketState::FellMoreThanMarket => "fell_more_than_market",
                MarketState::MovedWithMarket => "moved_with_market",
                MarketState::FellLessThanMarket => "fell_less_than_market",
                MarketState::Unknown => "unknown",
            },
            reasons: reasons(v.reasons),
            values: MarketValues {
                source,
                index_code,
                index_name,
                index_start_date,
                index_end_date,
                change_percent: money_string(v.change_percent),
                reference_percent: percent(v.reference_percent),
                difference_percent: percent(v.difference_percent),
                margin_percent: money_string(v.margin_percent),
                count: v.count,
            },
        }
    }
}

#[derive(Debug, Serialize)]
pub struct StoppedFallingValues {
    latest_close: Option<String>,
    low_close: Option<String>,
    low_date: Option<NaiveDate>,
    low_age_trading_days: Option<i64>,
}
impl From<StoppedFallingOutcome> for Check<StoppedFallingValues> {
    fn from(v: StoppedFallingOutcome) -> Self {
        Self {
            state: yes_no(v.state),
            reasons: reasons(v.reasons),
            values: StoppedFallingValues {
                latest_close: decimal(v.latest_close),
                low_close: decimal(v.low_close),
                low_date: v.low_date,
                low_age_trading_days: v.low_age_trading_days,
            },
        }
    }
}
#[derive(Debug, Serialize)]
pub struct DividendValues {
    events: Option<Vec<DividendEventResponse>>,
    amount: Option<String>,
    percent_of_start_close: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct DividendEventResponse {
    ex_date: NaiveDate,
    amount: String,
    currency: String,
}
impl From<DividendOutcome> for Check<DividendValues> {
    fn from(v: DividendOutcome) -> Self {
        // Unknown currency can retain selected events; earlier failures select none.
        let events_computed = v.state != YesNoUnknown::Unknown || !v.events.is_empty();
        Self {
            state: yes_no(v.state),
            reasons: reasons(v.reasons),
            values: DividendValues {
                events: events_computed.then(|| {
                    v.events
                        .into_iter()
                        .map(|e| DividendEventResponse {
                            ex_date: e.ex_date,
                            amount: e.amount.to_string(),
                            currency: e.currency,
                        })
                        .collect()
                }),
                amount: decimal(v.amount),
                percent_of_start_close: percent(v.percent_of_start_close),
            },
        }
    }
}
#[derive(Debug, Serialize)]
pub struct RoomValues {
    target_status: &'static str,
}
impl Check<RoomValues> {
    pub fn room(room: RoomToBuy, target: &ConvictionTargetOutput) -> Self {
        let state = match room {
            RoomToBuy::RoomToBuy => "room_to_buy",
            RoomToBuy::AlreadyAtTarget => "already_at_target",
            RoomToBuy::NoTarget => "no_target",
            RoomToBuy::Unknown => "unknown",
        };
        let reasons = if let TargetField::Unavailable(reasons) = &target.target_value {
            reasons
                .iter()
                .map(|r| Reason::new(r.as_str()).into())
                .collect()
        } else {
            vec![]
        };
        Self {
            state,
            reasons,
            values: RoomValues {
                target_status: target.status.as_str(),
            },
        }
    }
}
