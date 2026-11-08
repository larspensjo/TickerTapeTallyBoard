use super::{Figure, Reason, ReportedValue};
use crate::domain::calendar::month_end;
use crate::domain::fundamentals::{Metric, PeriodType};
use chrono::{Months, NaiveDate};
use rust_decimal::Decimal;
use std::collections::BTreeSet;

#[derive(Clone, Debug)]
pub struct Quarter {
    pub period_end: NaiveDate,
    pub missing: bool,
    pub sales_missing: bool,
    pub eps_missing: bool,
    pub currency: Option<String>,
    pub sales: Figure,
    pub eps: Figure,
    pub net_income: Figure,
    pub shares: Option<Decimal>,
}

fn shift(date: NaiveDate, months: i32) -> NaiveDate {
    month_end(
        if months < 0 {
            date.checked_sub_months(Months::new(months.unsigned_abs()))
        } else {
            date.checked_add_months(Months::new(months as u32))
        }
        .expect("supported quarter date"),
    )
    .expect("supported quarter month end")
}
pub fn quarter_grid(values: &[ReportedValue], splits: &[NaiveDate]) -> Vec<Quarter> {
    build_grid(values, splits, false)
}

pub(super) fn trailing_quarters(values: &[ReportedValue], splits: &[NaiveDate]) -> Vec<Quarter> {
    build_grid(values, splits, true)
}

fn build_grid(values: &[ReportedValue], splits: &[NaiveDate], trailing_only: bool) -> Vec<Quarter> {
    let ends: BTreeSet<_> = values
        .iter()
        .filter(|v| v.input.period_type == PeriodType::Quarter)
        .map(|v| v.input.period_end)
        .collect();
    // Near-duplicate period ends cannot represent two quarters. Prefer the later
    // end defensively, including when a source supplies a corrected period end.
    let mut distinct = Vec::new();
    for end in ends.into_iter().rev() {
        if distinct
            .last()
            .is_none_or(|later: &NaiveDate| (*later - end).num_days() >= 80)
        {
            distinct.push(end);
        }
    }
    let ends: BTreeSet<_> = distinct.into_iter().collect();
    let mut dates: Vec<NaiveDate> = Vec::new();
    for end in &ends {
        if let Some(previous) = dates.last().copied() {
            let gap = (*end - previous).num_days();
            let steps = if (80..=100).contains(&gap) {
                1
            } else {
                ((gap as f64 / 91.3).round() as i32).max(1)
            };
            for n in 1..steps {
                dates.push(shift(previous, n * 3));
            }
        }
        dates.push(*end);
    }
    let get = |date: NaiveDate, metric: Metric, period: PeriodType| {
        values
            .iter()
            .find(|v| {
                v.input.period_end == date
                    && v.input.metric == metric
                    && v.input.period_type == period
            })
            .filter(|v| {
                !matches!(metric, Metric::DilutedEps | Metric::DilutedShares)
                    || !splits.iter().any(|s| *s > date && *s > v.fetched_on)
            })
    };
    let additive = |date: NaiveDate, metric: Metric| -> Option<(Decimal, Option<String>)> {
        if let Some(v) = get(date, metric, PeriodType::Quarter) {
            return Some((v.input.value, v.input.currency.clone()));
        }
        // Annual data covers exactly four grid slots ending at an actually stored
        // quarter. A same-measure annual value can fill precisely one missing slot.
        for annual in values.iter().filter(|v| {
            v.input.metric == metric
                && v.input.period_type == PeriodType::Annual
                && ends.contains(&v.input.period_end)
        }) {
            let Some(last) = dates.iter().position(|d| *d == annual.input.period_end) else {
                continue;
            };
            if last < 3 {
                continue;
            }
            let window = &dates[last - 3..=last];
            if !window.contains(&date)
                || (window[3] - window[0]).num_days() < 240
                || (window[3] - window[0]).num_days() > 300
            {
                continue;
            }
            let others: Option<Vec<_>> = window
                .iter()
                .filter(|d| **d != date)
                .map(|d| {
                    get(*d, metric, PeriodType::Quarter)
                        .filter(|v| v.input.currency == annual.input.currency)
                })
                .collect();
            if let Some(others) = others {
                return Some((
                    annual.input.value - others.iter().map(|v| v.input.value).sum::<Decimal>(),
                    annual.input.currency.clone(),
                ));
            }
        }
        None
    };
    dates
        .iter()
        .skip(if trailing_only {
            dates.len().saturating_sub(5)
        } else {
            0
        })
        .map(|date| {
            let revenue = additive(*date, Metric::Revenue);
            let income = additive(*date, Metric::DilutedNetIncome)
                .or_else(|| additive(*date, Metric::CommonNetIncome));
            let shares =
                get(*date, Metric::DilutedShares, PeriodType::Quarter).map(|v| v.input.value);
            let reported_eps = get(*date, Metric::DilutedEps, PeriodType::Quarter);
            let eps = if let Some(v) = reported_eps {
                Figure::available(v.input.value, vec![])
            } else if let (Some((n, _)), Some(s)) = (&income, shares.filter(|s| *s > Decimal::ZERO))
            {
                Figure::available(*n / s, vec![Reason::new("eps_derived_from_net_income")])
            } else {
                Figure::unavailable("quarter_missing")
            };
            let currencies: BTreeSet<_> = revenue
                .as_ref()
                .and_then(|v| v.1.clone())
                .into_iter()
                .chain(income.as_ref().and_then(|v| v.1.clone()))
                .chain(reported_eps.and_then(|v| v.input.currency.clone()))
                .collect();
            let currency = currencies.iter().next().cloned();
            let changed = currencies.len() > 1;
            let figure = |v: Option<(Decimal, Option<String>)>| {
                if changed {
                    Figure::unavailable("report_currency_changed")
                } else {
                    v.map(|v| Figure::available(v.0, vec![]))
                        .unwrap_or_else(|| Figure::unavailable("quarter_missing"))
                }
            };
            let sales = figure(revenue);
            let eps = if changed {
                Figure::unavailable("report_currency_changed")
            } else {
                eps
            };
            Quarter {
                period_end: *date,
                missing: !ends.contains(date),
                sales_missing: sales.value.is_none(),
                eps_missing: eps.value.is_none(),
                currency,
                sales,
                net_income: figure(income),
                shares,
                eps,
            }
        })
        .collect()
}

pub(super) fn ttm<'a>(slots: impl Iterator<Item = &'a Figure>) -> Figure {
    let slots: Vec<_> = slots.collect();
    if let Some(f) = slots.iter().find(|f| {
        f.reasons
            .iter()
            .any(|r| r.code == "report_currency_changed")
    }) {
        return (*f).clone();
    }
    let usable: Vec<_> = slots.iter().filter(|f| f.value.is_some()).collect();
    if usable.len() < 3 {
        return Figure::unavailable("too_few_quarters");
    }
    let mut reasons = Vec::new();
    for f in &usable {
        for reason in &f.reasons {
            if !reasons.contains(reason) {
                reasons.push(reason.clone());
            }
        }
    }
    let mut sum: Decimal = usable.iter().filter_map(|f| f.value).sum();
    if usable.len() == 3 {
        sum = sum * Decimal::from(4) / Decimal::from(3);
        reasons.push(Reason::new("ttm_from_three_quarters"));
    }
    Figure::available(sum, reasons)
}
