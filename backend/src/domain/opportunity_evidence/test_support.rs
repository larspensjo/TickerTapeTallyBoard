use crate::domain::{
    company_figures::{self, CompanyFigures, Inputs, ReportedValue, SourceState},
    fundamentals::{Classification, FundamentalValue, LinkStatus, Metric, PeriodType},
    report_dates::{EstimateBasis, ReportCertainty, ReportDate},
};
use chrono::NaiveDate;
use rust_decimal_macros::dec;

pub fn date(s: &str) -> NaiveDate {
    s.parse().unwrap()
}
pub fn today() -> NaiveDate {
    date("2026-11-11")
}
pub fn source() -> SourceState {
    SourceState {
        classification: Classification::Equity,
        status: LinkStatus::Usable,
        awaiting_figures: false,
        unreachable: false,
        last_success: None,
    }
}
pub fn values() -> Vec<ReportedValue> {
    [
        "2025-09-30",
        "2025-12-31",
        "2026-03-31",
        "2026-06-30",
        "2026-09-30",
    ]
    .into_iter()
    .flat_map(|end| {
        [
            (Metric::Revenue, dec!(100)),
            (Metric::DilutedNetIncome, dec!(10)),
            (Metric::DilutedEps, dec!(1)),
            (Metric::DilutedShares, dec!(10)),
        ]
        .into_iter()
        .map(move |(metric, value)| ReportedValue {
            input: FundamentalValue {
                period_type: PeriodType::Quarter,
                period_end: date(end),
                metric,
                value,
                currency: Some("SEK".into()),
            },
            fetched_on: today(),
        })
    })
    .collect()
}
pub fn figures() -> CompanyFigures {
    company_figures::compute(&Inputs {
        today: today(),
        values: &values(),
        splits: &[],
        price: None,
        evidence_price: None,
        fx: &[],
        source: &source(),
        next_report: None,
    })
}
pub fn report(date: Option<NaiveDate>, basis: Option<EstimateBasis>) -> ReportDate {
    ReportDate {
        certainty: if basis == Some(EstimateBasis::CompanyAnnounced) {
            ReportCertainty::Confirmed
        } else {
            ReportCertainty::Estimated
        },
        basis,
        date,
        days_until: date.map(|d| (d - today()).num_days()),
        period_end: None,
        observed_between: None,
        reasons: vec![],
    }
}
