use super::YesNoUnknown;
use crate::domain::company_figures::Reason;
use chrono::NaiveDate;
use rust_decimal::Decimal;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DividendEvent {
    pub ex_date: NaiveDate,
    /// Validated positive amount in the event's currency.
    pub amount: Decimal,
    pub currency: String,
}

#[derive(Clone, Debug)]
pub struct DividendCoverage {
    pub start: NaiveDate,
    pub end: NaiveDate,
    pub valid: bool,
}

#[derive(Clone, Debug)]
pub struct DividendOutcome {
    pub state: YesNoUnknown,
    pub reasons: Vec<Reason>,
    pub events: Vec<DividendEvent>,
    pub amount: Option<Decimal>,
    pub percent_of_start_close: Option<Decimal>,
}

/// Coverage describes the latest capture for each supplied range, including failed
/// validation. Failures override retained events and older successful coverage.
pub fn went_ex_dividend(
    start: NaiveDate,
    end: NaiveDate,
    start_close: Decimal,
    start_close_currency: &str,
    source_reports_dividends: bool,
    events: &[DividendEvent],
    coverage: &[DividendCoverage],
) -> DividendOutcome {
    let mut out = DividendOutcome {
        state: YesNoUnknown::Unknown,
        reasons: vec![],
        events: vec![],
        amount: None,
        percent_of_start_close: None,
    };
    let reason = if !source_reports_dividends {
        Some("dividends_not_reported")
    } else if start >= end || start_close <= Decimal::ZERO {
        Some("invalid_dividend_interval")
    } else if coverage
        .iter()
        .any(|c| !c.valid && c.start <= end && c.end > start)
    {
        Some("dividend_capture_invalid")
    } else {
        None
    };
    if let Some(code) = reason {
        out.reasons.push(Reason::new(code));
        return out;
    }
    // Adjacent successful captures may jointly cover the interval.
    let mut ranges: Vec<_> = coverage
        .iter()
        .filter(|c| c.valid && c.start <= c.end)
        .collect();
    ranges.sort_by_key(|c| c.start);
    let mut covered_until = start;
    for range in ranges {
        if range.start.signed_duration_since(covered_until).num_days() > 1 {
            break;
        }
        covered_until = covered_until.max(range.end);
    }
    if covered_until < end {
        out.reasons.push(Reason::new("dividend_coverage_missing"));
        return out;
    }
    out.events = events
        .iter()
        .filter(|e| e.ex_date > start && e.ex_date <= end)
        .cloned()
        .collect();
    if out
        .events
        .iter()
        .any(|e| e.currency != start_close_currency)
    {
        out.reasons.push(Reason::new("dividend_currency_mismatch"));
        return out;
    }
    out.state = if out.events.is_empty() {
        YesNoUnknown::No
    } else {
        YesNoUnknown::Yes
    };
    let amount: Decimal = out.events.iter().map(|e| e.amount).sum();
    out.amount = Some(amount);
    out.percent_of_start_close = Some(amount / start_close * Decimal::ONE_HUNDRED);
    out
}

#[cfg(test)]
mod tests;
