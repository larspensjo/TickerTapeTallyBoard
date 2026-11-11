//! Company figures from source-neutral reported inputs. No I/O or provider ratios.
mod eps_consistency;
mod quarters;
pub use eps_consistency::*;
#[cfg(test)]
mod tests;

use super::fundamentals::{Classification, FundamentalValue, LinkStatus, Metric};
use chrono::NaiveDate;
pub use quarters::{quarter_grid, Quarter};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reason {
    pub code: &'static str,
    pub detail: Option<String>,
}
impl Reason {
    pub fn new(code: &'static str) -> Self {
        Self { code, detail: None }
    }
    pub fn detail(code: &'static str, detail: impl ToString) -> Self {
        Self {
            code,
            detail: Some(detail.to_string()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Certainty {
    Complete,
    Approximate,
    NotApplicable,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Figure {
    pub certainty: Certainty,
    pub value: Option<Decimal>,
    pub reasons: Vec<Reason>,
}
impl Figure {
    pub fn unavailable(code: &'static str) -> Self {
        Self::missing(Reason::new(code))
    }
    pub fn missing(reason: Reason) -> Self {
        Self {
            certainty: Certainty::Unavailable,
            value: None,
            reasons: vec![reason],
        }
    }
    pub fn available(value: Decimal, reasons: Vec<Reason>) -> Self {
        Self {
            certainty: if reasons.is_empty() {
                Certainty::Complete
            } else {
                Certainty::Approximate
            },
            value: Some(value),
            reasons,
        }
    }
    fn warn(&mut self, reason: Reason) {
        if self.value.is_some() {
            self.certainty = Certainty::Approximate;
        }
        if !self.reasons.contains(&reason) {
            self.reasons.push(reason);
        }
    }
}

#[derive(Clone, Debug)]
pub struct ReportedValue {
    pub input: FundamentalValue,
    pub fetched_on: NaiveDate,
}
#[derive(Clone, Debug)]
pub struct Price {
    pub date: NaiveDate,
    pub close: Decimal,
    pub currency: String,
    pub stale: bool,
}
#[derive(Clone, Debug)]
pub struct FxRate {
    pub currency: String,
    pub date: NaiveDate,
    pub sek_per_unit: Decimal,
}
#[derive(Clone, Debug)]
pub struct SourceState {
    pub classification: Classification,
    pub status: LinkStatus,
    pub awaiting_figures: bool,
    pub unreachable: bool,
    pub last_success: Option<NaiveDate>,
}
impl SourceState {
    pub fn blocking_reason(&self, has_figures: bool) -> Option<Reason> {
        if let Some(code) = self.not_applicable() {
            return Some(Reason::new(code));
        }
        if self.awaiting_figures {
            return Some(Reason::new("awaiting_figures"));
        }
        // A same-company correction resets discovery bookkeeping, but the retained
        // active quarters remain usable independently of the replacement link.
        if has_figures {
            return None;
        }
        match self.status {
            LinkStatus::NoMatch | LinkStatus::Pending => {
                Some(Reason::new("no_fundamentals_source"))
            }
            LinkStatus::Ambiguous => Some(Reason::new("source_ambiguous")),
            _ if self.classification == Classification::Unknown => {
                Some(Reason::new("security_type_unknown"))
            }
            _ => None,
        }
    }
    fn outage_reason(&self, today: NaiveDate, has_figures: bool) -> Option<Reason> {
        (self.unreachable
            && (!has_figures || self.last_success.is_none_or(|d| (today - d).num_days() > 7)))
        .then(|| Reason {
            code: "source_unreachable",
            detail: self.last_success.map(|d| d.to_string()),
        })
    }
    pub fn not_applicable(&self) -> Option<&'static str> {
        match self.classification {
            Classification::Fund => Some("fund_or_etf"),
            Classification::Certificate => Some("certificate"),
            Classification::NotCompany => Some("not_a_company"),
            _ => None,
        }
    }
}

pub struct Inputs<'a> {
    pub today: NaiveDate,
    pub values: &'a [ReportedValue],
    pub splits: &'a [NaiveDate],
    pub price: Option<&'a Price>,
    pub evidence_price: Option<&'a Price>,
    pub fx: &'a [FxRate],
    pub source: &'a SourceState,
    pub next_report: Option<super::report_dates::ReportExpectation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShareBasisState {
    Established,
    ListingUnitMismatch,
    Unverified,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ShareBasis {
    pub state: ShareBasisState,
    pub ratio: Option<Decimal>,
    pub evidence_date: Option<NaiveDate>,
}
impl ShareBasis {
    fn reason(&self) -> Reason {
        match self.ratio {
            Some(r) if self.state == ShareBasisState::ListingUnitMismatch => {
                Reason::detail("listing_unit_mismatch", r)
            }
            _ => Reason::new("share_basis_unverified"),
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct FxConversion {
    pub from: String,
    pub to: String,
    pub rate: Decimal,
    /// The date at which the conversion is resolved; component dates expose carry-forward.
    pub date: NaiveDate,
    pub from_rate_date: NaiveDate,
    pub to_rate_date: NaiveDate,
}
#[derive(Clone, Debug)]
pub struct CompanyFigures {
    pub pe: Figure,
    pub ps: Figure,
    pub profit_margin: Figure,
    pub sales_growth: Figure,
    pub fx_conversion: Option<FxConversion>,
    pub share_basis: ShareBasis,
    pub quarters: Vec<Quarter>,
    pub eps_consistency: EpsConsistency,
}

// Verified ECB currencies. An arbitrary cached rate must not enable an unsupported currency.
pub use super::fundamentals::ECB_CURRENCIES;
pub fn fx_cross(
    from: &str,
    to: &str,
    date: NaiveDate,
    rates: &[FxRate],
) -> Result<FxConversion, Reason> {
    let rate = |currency: &str| {
        if !ECB_CURRENCIES.contains(&currency) {
            return Err(Reason::detail("fx_unavailable", currency));
        }
        if currency == "SEK" {
            return Ok((Decimal::ONE, date));
        }
        rates
            .iter()
            .filter(|r| r.currency == currency && r.date <= date && r.sek_per_unit > Decimal::ZERO)
            .max_by_key(|r| r.date)
            .map(|r| (r.sek_per_unit, r.date))
            .ok_or_else(|| Reason::detail("fx_unavailable", currency))
    };
    let (a, ad) = rate(from)?;
    let (b, bd) = rate(to)?;
    Ok(FxConversion {
        from: from.into(),
        to: to.into(),
        rate: a / b,
        date,
        from_rate_date: ad,
        to_rate_date: bd,
    })
}

fn share_basis(input: &Inputs<'_>, grid: &[Quarter]) -> ShareBasis {
    let evidence = input
        .values
        .iter()
        .filter(|v| v.input.metric == Metric::MarketCap)
        .max_by_key(|v| v.input.period_end);
    let mut result = ShareBasis {
        state: ShareBasisState::Unverified,
        ratio: None,
        evidence_date: evidence.map(|v| v.input.period_end),
    };
    let (Some(e), Some(price), Some(shares)) = (
        evidence,
        input.evidence_price,
        grid.iter().rev().take(4).find_map(|q| q.shares),
    ) else {
        return result;
    };
    if !(0..=30).contains(&(input.today - e.input.period_end).num_days())
        || price.date > e.input.period_end
        || price.close <= Decimal::ZERO
        || shares <= Decimal::ZERO
        || e.input.value <= Decimal::ZERO
    {
        return result;
    }
    let currency = e.input.currency.as_deref().or_else(|| {
        let currencies: Vec<_> = input
            .values
            .iter()
            .filter(|v| v.input.metric == Metric::MarketCap)
            .filter_map(|v| v.input.currency.as_deref())
            .collect();
        (!currencies.is_empty() && currencies.iter().all(|c| *c == price.currency))
            .then_some(price.currency.as_str())
    });
    let Some(currency) = currency else {
        return result;
    };
    let cross = if price.currency == currency {
        Decimal::ONE
    } else {
        let Ok(fx) = fx_cross(&price.currency, currency, e.input.period_end, input.fx) else {
            return result;
        };
        fx.rate
    };
    let units = e.input.value / (price.close * shares * cross);
    if units <= Decimal::ZERO {
        return result;
    }
    if (dec!(0.80)..=dec!(1.25)).contains(&units) {
        result.state = ShareBasisState::Established;
        return result;
    }
    // Multiplicative distance is monotonic with absolute log distance, without float rounding.
    let nearest = (2..=20)
        .flat_map(|n| [Decimal::from(n), Decimal::ONE / Decimal::from(n)])
        .min_by_key(|r| if units > *r { units / *r } else { *r / units });
    if let Some(r) = nearest.filter(|r| (dec!(0.90)..=dec!(1.10)).contains(&(units / *r))) {
        result.state = ShareBasisState::ListingUnitMismatch;
        result.ratio = Some(r.round_dp(4).normalize());
    }
    result
}

pub fn compute(input: &Inputs<'_>) -> CompanyFigures {
    compute_grid(input, false)
}

/// Reuse the ratio rules while assembling only the trailing calculation window.
/// Historical chart quarters belong to the stock-page read path.
pub fn compute_holdings(input: &Inputs<'_>) -> CompanyFigures {
    compute_grid(input, true)
}

fn compute_grid(input: &Inputs<'_>, trailing_only: bool) -> CompanyFigures {
    let splits: Vec<_> = input
        .splits
        .iter()
        .copied()
        .filter(|s| *s <= input.price.map_or(input.today, |p| p.date))
        .collect();
    let grid = if trailing_only {
        quarters::trailing_quarters(input.values, &splits)
    } else {
        quarter_grid(input.values, &splits)
    };
    let basis = share_basis(input, &grid);
    let eps_consistency = evaluate_eps_consistency(&grid[grid.len().saturating_sub(4)..], 3);
    let mut result = CompanyFigures {
        pe: Figure::unavailable("too_few_quarters"),
        ps: Figure::unavailable("too_few_quarters"),
        profit_margin: Figure::unavailable("too_few_quarters"),
        sales_growth: Figure::unavailable("year_ago_quarter_missing"),
        fx_conversion: None,
        share_basis: basis,
        quarters: grid,
        eps_consistency,
    };
    if let Some(reason) = input.source.blocking_reason(!result.quarters.is_empty()) {
        let mut figure = Figure::missing(reason);
        if input.source.not_applicable().is_some() {
            figure.certainty = Certainty::NotApplicable;
        } else if let Some(reason) = input.source.outage_reason(input.today, false) {
            figure.warn(reason);
        }
        result.pe = figure.clone();
        result.ps = figure.clone();
        result.profit_margin = figure.clone();
        result.sales_growth = figure;
        return result;
    }
    if result.quarters.is_empty() {
        let figure = Figure::missing(
            input
                .source
                .outage_reason(input.today, false)
                .unwrap_or_else(|| Reason::new("too_few_quarters")),
        );
        result.pe = figure.clone();
        result.ps = figure.clone();
        result.profit_margin = figure.clone();
        result.sales_growth = figure;
        return result;
    }
    let grid = &result.quarters;
    let window = &grid[grid.len().saturating_sub(4)..];
    let revenue = quarters::ttm(window.iter().map(|q| &q.sales));
    let eps = quarters::ttm(window.iter().map(|q| &q.eps));
    let paired: Vec<_> = window
        .iter()
        .filter(|q| q.sales.value.is_some() && q.net_income.value.is_some())
        .collect();
    result.profit_margin = divide(
        &quarters::ttm(paired.iter().map(|q| &q.net_income)),
        &quarters::ttm(paired.iter().map(|q| &q.sales)),
        "non_positive_revenue",
    );
    if grid.len() >= 5 {
        result.sales_growth = if grid[grid.len() - 5].sales.value.is_none() {
            Figure::unavailable("year_ago_quarter_missing")
        } else if grid[grid.len() - 1].currency != grid[grid.len() - 5].currency {
            Figure::unavailable("report_currency_changed")
        } else {
            divide(
                &grid[grid.len() - 1].sales,
                &grid[grid.len() - 5].sales,
                "non_positive_revenue",
            )
        };
        if let Some(v) = result.sales_growth.value.as_mut() {
            *v -= Decimal::ONE;
        }
    }
    let ratio_inputs = || -> Result<(Decimal, Decimal), Reason> {
        let price = input
            .price
            .filter(|p| p.close > Decimal::ZERO)
            .ok_or_else(|| Reason::new("price_unavailable"))?;
        if input.splits.iter().any(|s| {
            *s <= price.date
                && grid.last().is_some_and(|q| *s > q.period_end)
                && !(result.share_basis.state == ShareBasisState::Established
                    && result.share_basis.evidence_date.is_some_and(|e| e >= *s))
        }) {
            return Err(Reason::new("split_since_report"));
        }
        if result.share_basis.state != ShareBasisState::Established {
            return Err(result.share_basis.reason());
        }
        let currency = window
            .iter()
            .find_map(|q| q.currency.as_deref())
            .ok_or_else(|| Reason::new("too_few_quarters"))?;
        let fx = if currency == price.currency {
            Decimal::ONE
        } else {
            fx_cross(currency, &price.currency, price.date, input.fx)?.rate
        };
        Ok((price.close, fx))
    };
    match ratio_inputs() {
        Err(reason) => {
            result.pe = Figure::missing(reason.clone());
            result.ps = Figure::missing(reason);
        }
        Ok((price, fx)) => {
            result.pe = divide(&Figure::available(price, vec![]), &eps, "loss_making");
            if let Some(v) = result.pe.value.as_mut() {
                *v /= fx;
            }
            result.ps = match window
                .iter()
                .rev()
                .find_map(|q| q.shares)
                .filter(|s| *s > Decimal::ZERO)
            {
                Some(shares) => divide(
                    &Figure::available(price * shares / fx, vec![]),
                    &revenue,
                    "non_positive_revenue",
                ),
                None => Figure::unavailable("share_basis_unverified"),
            };
            if let (Some(p), Some(c)) = (
                input.price,
                window.iter().find_map(|q| q.currency.as_deref()),
            ) {
                if c != p.currency {
                    result.fx_conversion = fx_cross(c, &p.currency, p.date, input.fx).ok();
                }
            }
        }
    }
    if matches!(
        result.eps_consistency.state,
        EpsConsistencyState::Approximate | EpsConsistencyState::Inconsistent
    ) {
        let severe = result.eps_consistency.state == EpsConsistencyState::Inconsistent;
        for f in [&mut result.pe, &mut result.ps] {
            if f.value.is_none() {
                continue;
            }
            let reason = Reason::detail(
                if severe {
                    "figures_inconsistent"
                } else {
                    "figures_disagree"
                },
                format!(
                    "{:.1}%",
                    result
                        .eps_consistency
                        .gap_percent
                        .expect("evaluated gap")
                        .round_dp(1)
                ),
            );
            if severe {
                *f = Figure::missing(reason);
            } else {
                f.warn(reason);
            }
        }
    }
    for (f, bound) in [(&mut result.pe, dec!(1)), (&mut result.ps, dec!(0.05))] {
        if f.value.is_some_and(|v| v < bound) {
            *f = Figure::unavailable("implausible_ratio");
        }
        if input.price.is_some_and(|p| p.stale) {
            f.warn(Reason::new("price_stale"));
        }
    }
    let currency_changed = window
        .iter()
        .filter_map(|q| q.currency.as_ref())
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        > 1;
    for f in [
        &mut result.pe,
        &mut result.ps,
        &mut result.profit_margin,
        &mut result.sales_growth,
    ] {
        if grid
            .last()
            .is_some_and(|q| (input.today - q.period_end).num_days() > 200)
        {
            *f = Figure::unavailable("latest_quarter_too_old");
        }
        if currency_changed {
            *f = Figure::unavailable("report_currency_changed");
        }
        if input.next_report.is_some_and(|r| r.is_overdue(input.today)) {
            f.warn(Reason::new("newer_report_expected"));
        }
        if let Some(reason) = input.source.outage_reason(input.today, !grid.is_empty()) {
            f.warn(reason);
        }
    }
    if result.pe.value.is_none() && result.ps.value.is_none() {
        result.fx_conversion = None;
    }
    result
}

fn divide(numerator: &Figure, denominator: &Figure, non_positive: &'static str) -> Figure {
    match (numerator.value, denominator.value) {
        (_, Some(d)) if d <= Decimal::ZERO => Figure::unavailable(non_positive),
        (Some(n), Some(d)) => {
            let mut reasons = numerator.reasons.clone();
            for r in &denominator.reasons {
                if !reasons.contains(r) {
                    reasons.push(r.clone());
                }
            }
            Figure::available(n / d, reasons)
        }
        _ => {
            let mut f = if numerator.value.is_none() {
                numerator.clone()
            } else {
                denominator.clone()
            };
            f.value = None;
            f.certainty = Certainty::Unavailable;
            f
        }
    }
}
