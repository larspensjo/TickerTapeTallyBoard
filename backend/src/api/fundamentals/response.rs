use crate::domain::{company_figures as figures, report_dates as reports};
use chrono::NaiveDate;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ReasonResponse {
    pub code: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}
impl From<figures::Reason> for ReasonResponse {
    fn from(reason: figures::Reason) -> Self {
        Self {
            code: reason.code,
            detail: reason.detail,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct FigureResponse {
    pub certainty: &'static str,
    pub value: Option<String>,
    pub reasons: Vec<ReasonResponse>,
}
impl From<figures::Figure> for FigureResponse {
    fn from(figure: figures::Figure) -> Self {
        Self {
            certainty: match figure.certainty {
                figures::Certainty::Complete => "complete",
                figures::Certainty::Approximate => "approximate",
                figures::Certainty::NotApplicable => "not_applicable",
                figures::Certainty::Unavailable => "unavailable",
            },
            value: figure.value.map(|v| v.to_string()),
            reasons: figure.reasons.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ReportDateResponse {
    pub certainty: &'static str,
    pub basis: Option<&'static str>,
    pub date: Option<NaiveDate>,
    pub days_until: Option<i64>,
    pub period_end: Option<NaiveDate>,
    pub observed_between: Option<[NaiveDate; 2]>,
    pub reasons: Vec<ReasonResponse>,
}
impl From<reports::ReportDate> for ReportDateResponse {
    fn from(report: reports::ReportDate) -> Self {
        Self {
            certainty: match report.certainty {
                reports::ReportCertainty::Confirmed => "confirmed",
                reports::ReportCertainty::Estimated => "estimated",
                reports::ReportCertainty::Observed => "observed",
                reports::ReportCertainty::PeriodOnly => "period_only",
                reports::ReportCertainty::NotApplicable => "not_applicable",
                reports::ReportCertainty::Unavailable => "unavailable",
            },
            basis: report.basis.map(|basis| match basis {
                reports::EstimateBasis::CompanyAnnounced => "company_announced",
                reports::EstimateBasis::NasdaqExpected => "nasdaq_expected",
                reports::EstimateBasis::LastYearDate => "last_year_date",
                reports::EstimateBasis::TypicalGap => "typical_gap",
            }),
            date: report.date,
            days_until: report.days_until,
            period_end: report.period_end,
            observed_between: report.observed_between,
            reasons: report.reasons.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct QuarterResponse {
    pub period_end: NaiveDate,
    pub missing: bool,
    pub sales_missing: bool,
    pub eps_missing: bool,
    pub currency: Option<String>,
    pub sales: FigureResponse,
    pub eps: FigureResponse,
}
impl From<figures::Quarter> for QuarterResponse {
    fn from(quarter: figures::Quarter) -> Self {
        Self {
            period_end: quarter.period_end,
            missing: quarter.missing,
            sales_missing: quarter.sales_missing,
            eps_missing: quarter.eps_missing,
            currency: quarter.currency,
            sales: quarter.sales.into(),
            eps: quarter.eps.into(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ShareBasisResponse {
    pub state: &'static str,
    pub ratio: Option<String>,
    pub evidence_date: Option<NaiveDate>,
}
impl From<figures::ShareBasis> for ShareBasisResponse {
    fn from(basis: figures::ShareBasis) -> Self {
        Self {
            state: match basis.state {
                figures::ShareBasisState::Established => "established",
                figures::ShareBasisState::ListingUnitMismatch => "listing_unit_mismatch",
                figures::ShareBasisState::Unverified => "share_basis_unverified",
            },
            ratio: basis.ratio.map(|v| v.to_string()),
            evidence_date: basis.evidence_date,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct FxConversionResponse {
    pub from: String,
    pub to: String,
    pub rate: String,
    pub date: NaiveDate,
    pub from_rate_date: NaiveDate,
    pub to_rate_date: NaiveDate,
}
impl From<figures::FxConversion> for FxConversionResponse {
    fn from(fx: figures::FxConversion) -> Self {
        Self {
            from: fx.from,
            to: fx.to,
            rate: fx.rate.to_string(),
            date: fx.date,
            from_rate_date: fx.from_rate_date,
            to_rate_date: fx.to_rate_date,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct CompanyFiguresResponse {
    pub pe: FigureResponse,
    pub ps: FigureResponse,
    pub profit_margin: FigureResponse,
    pub sales_growth: FigureResponse,
    pub fx_conversion: Option<FxConversionResponse>,
    pub share_basis: ShareBasisResponse,
    pub quarters: Vec<QuarterResponse>,
}
impl From<figures::CompanyFigures> for CompanyFiguresResponse {
    fn from(figures: figures::CompanyFigures) -> Self {
        Self {
            pe: figures.pe.into(),
            ps: figures.ps.into(),
            profit_margin: figures.profit_margin.into(),
            sales_growth: figures.sales_growth.into(),
            fx_conversion: figures.fx_conversion.map(Into::into),
            share_basis: figures.share_basis.into(),
            quarters: figures.quarters.into_iter().map(Into::into).collect(),
        }
    }
}
