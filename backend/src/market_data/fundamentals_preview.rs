use super::provider_registry::ProviderSet;
use crate::{
    domain::fundamentals::PeriodType,
    providers::{
        yahoo_fundamentals::{FundamentalsRequest, TIMESERIES_ENDPOINT},
        MarketDataProvider,
    },
};
use chrono::NaiveDate;
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewStatus {
    Found,
    NoMatch,
    ProviderUnavailable,
}

#[derive(Debug, Serialize)]
pub struct FundamentalsPreview {
    pub status: PreviewStatus,
    pub symbol: String,
    pub name: Option<String>,
    pub security_type: Option<String>,
    pub report_currency: Option<String>,
    pub quarters: usize,
    pub latest_period_end: Option<NaiveDate>,
}
impl FundamentalsPreview {
    pub fn unavailable(symbol: String) -> Self {
        Self {
            status: PreviewStatus::ProviderUnavailable,
            symbol,
            name: None,
            security_type: None,
            report_currency: None,
            quarters: 0,
            latest_period_end: None,
        }
    }
}

pub(super) async fn preview(
    providers: &ProviderSet,
    id: i64,
    symbol: String,
    today: NaiveDate,
) -> FundamentalsPreview {
    let mut result = FundamentalsPreview::unavailable(symbol.clone());
    let (Some(search), Some(figures)) = (
        providers.symbol_search_provider(MarketDataProvider::Yahoo),
        providers.fundamentals_provider(),
    ) else {
        return result;
    };
    let matches = match search.search(&symbol).await {
        Ok(matches) => matches,
        Err(error) => {
            crate::engine_warn!("fundamentals preview instrument_id={} symbol={} endpoint=yahoo_search reason={} message={}", id, symbol, error.reason_code(), error.message());
            return result;
        }
    };
    let Some(found) = matches.into_iter().find(|m| {
        m.provider == MarketDataProvider::Yahoo && m.provider_symbol.eq_ignore_ascii_case(&symbol)
    }) else {
        result.status = PreviewStatus::NoMatch;
        return result;
    };
    result.name = found.name;
    result.security_type = found.quote_type;
    let values = match figures
        .fetch(&FundamentalsRequest { symbol, end: today })
        .await
    {
        Ok(values) => values,
        Err(error) => {
            crate::engine_warn!(
                "fundamentals preview instrument_id={} symbol={} endpoint={} reason={} message={}",
                id,
                result.symbol,
                TIMESERIES_ENDPOINT,
                error.reason_code(),
                error.message()
            );
            return result;
        }
    };
    let quarters: std::collections::BTreeSet<_> = values
        .iter()
        .filter(|v| v.period_type == PeriodType::Quarter)
        .map(|v| v.period_end)
        .collect();
    result.quarters = quarters.len();
    result.latest_period_end = quarters.last().copied();
    result.report_currency = values
        .iter()
        .filter(|v| v.period_type == PeriodType::Quarter)
        .filter(|v| v.currency.is_some())
        .max_by_key(|v| v.period_end)
        .and_then(|v| v.currency.clone());
    result.status = PreviewStatus::Found;
    result
}
