//! Nasdaq date-only boundary. Analyst figures are deliberately ignored.
use super::{http::build_client, ProviderError, ProviderResult};
use crate::domain::{calendar::month_end, report_dates::*};
use async_trait::async_trait;
use chrono::NaiveDate;
use serde_json::Value;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;

pub const SYMBOL_ENDPOINT: &str = "https://api.nasdaq.com/api/analyst";
pub const CALENDAR_ENDPOINT: &str = "https://api.nasdaq.com/api/calendar/earnings";
const SOURCE: &str = "NASDAQ_US";
pub fn symbol_url(symbol: &str) -> String {
    format!("{SYMBOL_ENDPOINT}/{symbol}/earnings-date")
}
pub fn calendar_url(day: NaiveDate) -> String {
    format!("{CALENDAR_ENDPOINT}?date={day}")
}

#[async_trait]
pub trait EarningsCalendarProvider: Send + Sync {
    async fn report_date(&self, symbol: &str) -> ProviderResult<SymbolReportDate>;
    async fn calendar(
        &self,
        day: NaiveDate,
        today: NaiveDate,
    ) -> ProviderResult<Vec<CalendarSighting>>;
}

pub struct NasdaqEarningsCalendarClient {
    client: reqwest::Client,
}
impl Default for NasdaqEarningsCalendarClient {
    fn default() -> Self {
        Self::new()
    }
}
impl NasdaqEarningsCalendarClient {
    pub fn new() -> Self {
        Self {
            client: build_client(),
        }
    }
    async fn get(&self, url: String) -> ProviderResult<String> {
        let response = self
            .client
            .get(&url)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| ProviderError::transport(SOURCE, format!("{url}: {e}")))?;
        if !response.status().is_success() {
            return Err(ProviderError::with_http_status(
                SOURCE,
                response.status().as_u16(),
                url,
            ));
        }
        response
            .text()
            .await
            .map_err(|e| ProviderError::transport(SOURCE, format!("{url}: {e}")))
    }
    pub fn parse_report_date(body: &str) -> ProviderResult<SymbolReportDate> {
        let json = json(body)?;
        if json.pointer("/status/rCode").and_then(Value::as_i64) == Some(400)
            && json
                .pointer("/status/bCodeMessage")
                .and_then(Value::as_array)
                .is_some_and(|a| a.iter().any(|m| m["errorMessage"] == "Symbol not exists."))
        {
            return Ok(SymbolReportDate::UnknownSymbol);
        }
        success(&json)?;
        let text = json
            .pointer("/data/reportText")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("missing reportText"))?;
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.contains("hasn't provided us with the upcoming earnings report date") {
            return Ok(SymbolReportDate::None);
        }
        let algorithm = text.contains("is estimated to report earnings on")
            && text.contains("derived from an algorithm");
        let announced = text.contains("is expected* to report earnings on")
            || text.contains("is expected to report earnings on");
        if !algorithm && !announced {
            return Err(bad("unrecognised reportText form"));
        }
        let after = text
            .split_once("report earnings on")
            .ok_or_else(|| bad("missing report date"))?
            .1
            .trim_start();
        let raw = after.get(..10).ok_or_else(|| bad("short report date"))?;
        let date = NaiveDate::parse_from_str(raw, "%m/%d/%Y").map_err(|e| bad(e.to_string()))?;
        if algorithm {
            return Ok(SymbolReportDate::Dated(AnnouncementDate {
                date,
                kind: AnnouncementKind::Expected,
                fiscal_quarter: None,
                session: None,
            }));
        }
        let session = if after[10..].starts_with(" before market open.") {
            ReportSession::BeforeMarketOpen
        } else if after[10..].starts_with(" after market close.") {
            ReportSession::AfterMarketClose
        } else {
            return Err(bad("company form missing session"));
        };
        let quarter = text
            .split_once("fiscal Quarter ending ")
            .and_then(|(_, s)| s.split('.').next())
            .ok_or_else(|| bad("company form missing fiscal quarter"))?;
        Ok(SymbolReportDate::Dated(AnnouncementDate {
            date,
            kind: AnnouncementKind::CompanyAnnounced,
            fiscal_quarter: Some(fiscal_quarter(quarter)?),
            session: Some(session),
        }))
    }
    pub fn parse_calendar(
        body: &str,
        day: NaiveDate,
        today: NaiveDate,
    ) -> ProviderResult<Vec<CalendarSighting>> {
        let json = json(body)?;
        success(&json)?;
        let rows = json
            .pointer("/data/rows")
            .ok_or_else(|| bad("missing calendar rows"))?;
        if rows.is_null() {
            return Ok(vec![]);
        }
        rows.as_array()
            .ok_or_else(|| bad("invalid calendar rows"))?
            .iter()
            .map(|row| {
                let symbol = row["symbol"]
                    .as_str()
                    .ok_or_else(|| bad("missing calendar symbol"))?
                    .to_owned();
                let session = match row["time"].as_str() {
                    Some("time-pre-market") => Some(ReportSession::BeforeMarketOpen),
                    Some("time-after-hours") => Some(ReportSession::AfterMarketClose),
                    Some("time-not-supplied") => None,
                    _ => return Err(bad("unknown calendar session")),
                };
                let quarter = row["fiscalQuarterEnding"]
                    .as_str()
                    .filter(|s| !matches!(*s, "" | "N/A" | "--"))
                    .map(fiscal_quarter)
                    .transpose()?;
                Ok(CalendarSighting {
                    symbol,
                    report: AnnouncementDate {
                        date: day,
                        kind: if session.is_some() {
                            AnnouncementKind::CompanyAnnounced
                        } else {
                            AnnouncementKind::Expected
                        },
                        fiscal_quarter: quarter,
                        session,
                    },
                    presence_only: day <= today
                        || row.get("eps").is_some()
                        || row.get("surprise").is_some(),
                })
            })
            .collect()
    }
}
fn bad(message: impl Into<String>) -> ProviderError {
    ProviderError::provider_error(SOURCE, message)
}
fn json(body: &str) -> ProviderResult<Value> {
    serde_json::from_str(body).map_err(|e| bad(e.to_string()))
}
fn success(json: &Value) -> ProviderResult<()> {
    if json.pointer("/status/rCode").and_then(Value::as_i64) != Some(200) {
        return Err(bad("unsuccessful Nasdaq response"));
    }
    Ok(())
}
fn fiscal_quarter(raw: &str) -> ProviderResult<NaiveDate> {
    let normal = raw.trim().replace('/', " ");
    let day = NaiveDate::parse_from_str(&format!("01 {normal}"), "%d %b %Y")
        .map_err(|e| bad(format!("invalid fiscal quarter {raw}: {e}")))?;
    month_end(day).ok_or_else(|| bad("fiscal quarter out of range"))
}
#[async_trait]
impl EarningsCalendarProvider for NasdaqEarningsCalendarClient {
    async fn report_date(&self, symbol: &str) -> ProviderResult<SymbolReportDate> {
        Self::parse_report_date(&self.get(symbol_url(symbol)).await?)
    }
    async fn calendar(
        &self,
        day: NaiveDate,
        today: NaiveDate,
    ) -> ProviderResult<Vec<CalendarSighting>> {
        Self::parse_calendar(&self.get(calendar_url(day)).await?, day, today)
    }
}

#[derive(Default)]
struct FakeState {
    dates: VecDeque<ProviderResult<SymbolReportDate>>,
    calendars: VecDeque<ProviderResult<Vec<CalendarSighting>>>,
    symbols: Vec<String>,
    days: Vec<NaiveDate>,
    gate: Option<Arc<Notify>>,
}
#[derive(Clone, Default)]
pub struct FakeEarningsCalendarProvider {
    state: Arc<Mutex<FakeState>>,
}
impl FakeEarningsCalendarProvider {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn push_response(&self, response: ProviderResult<SymbolReportDate>) {
        self.state.lock().unwrap().dates.push_back(response);
    }
    pub fn push_calendar(&self, response: ProviderResult<Vec<CalendarSighting>>) {
        self.state.lock().unwrap().calendars.push_back(response);
    }
    pub fn calls(&self) -> Vec<String> {
        self.state.lock().unwrap().symbols.clone()
    }
    pub fn calendar_calls(&self) -> Vec<NaiveDate> {
        self.state.lock().unwrap().days.clone()
    }
    pub fn block_next_call_on(&self, gate: Arc<Notify>) {
        self.state.lock().unwrap().gate = Some(gate);
    }
}
#[async_trait]
impl EarningsCalendarProvider for FakeEarningsCalendarProvider {
    async fn report_date(&self, symbol: &str) -> ProviderResult<SymbolReportDate> {
        let (gate, response) = {
            let mut s = self.state.lock().unwrap();
            s.symbols.push(symbol.into());
            (
                s.gate.take(),
                s.dates
                    .pop_front()
                    .unwrap_or_else(|| Err(bad("no fake symbol response"))),
            )
        };
        if let Some(gate) = gate {
            gate.notified().await;
        }
        response
    }
    async fn calendar(
        &self,
        day: NaiveDate,
        _today: NaiveDate,
    ) -> ProviderResult<Vec<CalendarSighting>> {
        let mut s = self.state.lock().unwrap();
        s.days.push(day);
        s.calendars
            .pop_front()
            .unwrap_or_else(|| Err(bad("no fake calendar response")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(name: &str) -> String {
        std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/fundamentals")
                .join(name),
        )
        .unwrap()
    }
    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }
    #[test]
    fn per_symbol_forms_only_read_dates_and_company_marks() {
        let SymbolReportDate::Dated(company) = NasdaqEarningsCalendarClient::parse_report_date(
            &fixture("nasdaq_earnings_date_dsgx_company_announced.json"),
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(
            company,
            AnnouncementDate {
                date: d("2026-12-02"),
                kind: AnnouncementKind::CompanyAnnounced,
                fiscal_quarter: Some(d("2026-10-31")),
                session: Some(ReportSession::AfterMarketClose)
            }
        );
        let before = fixture("nasdaq_earnings_date_dsgx_company_announced.json")
            .replace("after market close", "before market open");
        assert!(matches!(
            NasdaqEarningsCalendarClient::parse_report_date(&before).unwrap(),
            SymbolReportDate::Dated(AnnouncementDate {
                session: Some(ReportSession::BeforeMarketOpen),
                ..
            })
        ));
        let SymbolReportDate::Dated(expected) = NasdaqEarningsCalendarClient::parse_report_date(
            &fixture("nasdaq_earnings_date_crm_algorithmic.json"),
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(expected.date, d("2026-12-02"));
        assert_eq!(expected.kind, AnnouncementKind::Expected);
        assert!(expected.fiscal_quarter.is_none());
        assert_eq!(
            NasdaqEarningsCalendarClient::parse_report_date(&fixture(
                "nasdaq_earnings_date_msft_not_provided.json"
            ))
            .unwrap(),
            SymbolReportDate::None
        );
        assert_eq!(
            NasdaqEarningsCalendarClient::parse_report_date(&fixture(
                "nasdaq_earnings_date_unknown_symbol.json"
            ))
            .unwrap(),
            SymbolReportDate::UnknownSymbol
        );
        assert!(NasdaqEarningsCalendarClient::parse_report_date("{}").is_err());
    }
    #[test]
    fn calendar_marks_results_presence_and_empty_days() {
        let upcoming = NasdaqEarningsCalendarClient::parse_calendar(
            &fixture("nasdaq_calendar_upcoming_day_trimmed.json"),
            d("2026-12-02"),
            d("2026-11-09"),
        )
        .unwrap();
        assert_eq!(upcoming.len(), 3);
        assert_eq!(upcoming[0].symbol, "CRM");
        assert_eq!(upcoming[0].report.kind, AnnouncementKind::Expected);
        assert_eq!(upcoming[2].symbol, "DSGX");
        assert_eq!(upcoming[2].report.kind, AnnouncementKind::CompanyAnnounced);
        assert_eq!(upcoming[2].report.fiscal_quarter, Some(d("2026-10-31")));
        let before = fixture("nasdaq_calendar_upcoming_day_trimmed.json")
            .replace("time-after-hours", "time-pre-market");
        assert_eq!(
            NasdaqEarningsCalendarClient::parse_calendar(&before, d("2026-12-02"), d("2026-11-09"))
                .unwrap()[2]
                .report
                .session,
            Some(ReportSession::BeforeMarketOpen)
        );
        let past = NasdaqEarningsCalendarClient::parse_calendar(
            &fixture("nasdaq_calendar_past_day_trimmed.json"),
            d("2026-10-28"),
            d("2026-11-09"),
        )
        .unwrap();
        assert!(past.iter().all(|s| s.presence_only));
        assert_eq!(past[0].report.fiscal_quarter, Some(d("2026-09-30")));
        assert!(NasdaqEarningsCalendarClient::parse_calendar(
            &fixture("nasdaq_calendar_empty_day.json"),
            d("2026-11-26"),
            d("2026-11-09")
        )
        .unwrap()
        .is_empty());
    }

    #[test]
    fn report_day_and_any_results_layout_are_presence_only() {
        for (fixture_name, day, today) in [
            (
                "nasdaq_calendar_upcoming_day_trimmed.json",
                "2026-12-02",
                "2026-12-02",
            ),
            (
                "nasdaq_calendar_upcoming_day_trimmed.json",
                "2026-12-02",
                "2026-12-03",
            ),
            (
                "nasdaq_calendar_past_day_trimmed.json",
                "2026-12-02",
                "2026-11-09",
            ),
        ] {
            let rows = NasdaqEarningsCalendarClient::parse_calendar(
                &fixture(fixture_name),
                d(day),
                d(today),
            )
            .unwrap();
            assert!(!rows.is_empty());
            assert!(rows.iter().all(|row| row.presence_only));
        }
    }
}
