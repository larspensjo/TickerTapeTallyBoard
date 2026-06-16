use reqwest::Client;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::Number;
use std::str::FromStr;

use super::{FxProvider, FxRate, ProviderError, ProviderMissingReason, ProviderResult};

const DEFAULT_BASE_URL: &str = "https://api.frankfurter.dev/v2/rates";

#[derive(Clone)]
pub struct FrankfurterClient {
    client: Client,
    base_url: String,
    provider_filter: Option<String>,
}

impl FrankfurterClient {
    pub fn new() -> Self {
        Self::with_provider_filter(Some("ECB".to_owned()))
    }

    pub fn with_provider_filter(provider_filter: Option<String>) -> Self {
        Self {
            client: build_client(),
            base_url: DEFAULT_BASE_URL.to_owned(),
            provider_filter,
        }
    }

    pub fn with_client(client: Client, provider_filter: Option<String>) -> Self {
        Self {
            client,
            base_url: DEFAULT_BASE_URL.to_owned(),
            provider_filter,
        }
    }

    fn url(
        &self,
        base: &str,
        quote: &str,
        start: chrono::NaiveDate,
        end: chrono::NaiveDate,
    ) -> String {
        let mut url = format!(
            "{}?base={base}&quotes={quote}&from={}&to={}",
            self.base_url,
            start.format("%Y-%m-%d"),
            end.format("%Y-%m-%d"),
        );
        if let Some(provider) = &self.provider_filter {
            url.push_str("&providers=");
            url.push_str(provider);
        }
        url
    }

    fn log_failure(
        base: &str,
        quote: &str,
        start: chrono::NaiveDate,
        end: chrono::NaiveDate,
        error: &ProviderError,
    ) {
        crate::engine_error!(
            "market data fx failure provider={} pair={}/{} range={}..{} reason={} message={}",
            FxProvider::Frankfurter,
            base,
            quote,
            start,
            end,
            error.reason_code(),
            error.message()
        );
    }

    fn parse_response(base: &str, quote: &str, body: &str) -> ProviderResult<Vec<FxRate>> {
        let rows: Vec<FrankfurterRateRow> = serde_json::from_str(body).map_err(|error| {
            ProviderError::provider_error(
                FxProvider::Frankfurter.as_str(),
                format!("failed to parse Frankfurter response for {base}/{quote}: {error}"),
            )
        })?;

        let mut parsed_rows: Vec<FxRate> = Vec::new();
        for row in rows {
            parsed_rows.push(FxRate {
                provider: FxProvider::Frankfurter,
                base: row.base,
                quote: row.quote,
                date: chrono::NaiveDate::parse_from_str(&row.date, "%Y-%m-%d").map_err(
                    |error| {
                        ProviderError::provider_error(
                            FxProvider::Frankfurter.as_str(),
                            format!(
                            "Frankfurter row for {base}/{quote} had an invalid date {:?}: {error}",
                            row.date
                        ),
                        )
                    },
                )?,
                rate: number_to_decimal(&row.rate, base, quote)?,
            });
        }

        if parsed_rows.is_empty() {
            return Err(ProviderError::new(
                FxProvider::Frankfurter.as_str(),
                ProviderMissingReason::NoDataInRange,
                format!("Frankfurter returned no rate rows for {base}/{quote}"),
            ));
        }

        parsed_rows.sort_by_key(|row| row.date);
        Ok(parsed_rows)
    }
}

impl Default for FrankfurterClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl super::FxRateProvider for FrankfurterClient {
    async fn fx_history(
        &self,
        base: &str,
        quote: &str,
        start: chrono::NaiveDate,
        end: chrono::NaiveDate,
    ) -> ProviderResult<Vec<FxRate>> {
        let url = self.url(base, quote, start, end);
        let response = match self.client.get(&url).send().await {
            Ok(response) => response,
            Err(error) => {
                let error = ProviderError::provider_error(
                    FxProvider::Frankfurter.as_str(),
                    format!("failed to request Frankfurter rates for {base}/{quote}: {error}"),
                );
                Self::log_failure(base, quote, start, end, &error);
                return Err(error);
            }
        };

        let status = response.status();
        let body = match response.text().await {
            Ok(body) => body,
            Err(error) => {
                let error = ProviderError::provider_error(
                    FxProvider::Frankfurter.as_str(),
                    format!("failed to read Frankfurter response for {base}/{quote}: {error}"),
                );
                Self::log_failure(base, quote, start, end, &error);
                return Err(error);
            }
        };

        if !status.is_success() {
            let error = ProviderError::with_http_status(
                FxProvider::Frankfurter.as_str(),
                status.as_u16(),
                format!("Frankfurter request for {base}/{quote} failed with HTTP {status}: {body}"),
            );
            Self::log_failure(base, quote, start, end, &error);
            return Err(error);
        }

        let parsed = Self::parse_response(base, quote, &body);
        if let Err(error) = &parsed {
            Self::log_failure(base, quote, start, end, error);
        }
        parsed
    }
}

fn number_to_decimal(number: &Number, base: &str, quote: &str) -> ProviderResult<Decimal> {
    Decimal::from_str(&number.to_string()).map_err(|error| {
        ProviderError::provider_error(
            FxProvider::Frankfurter.as_str(),
            format!("Frankfurter rate for {base}/{quote} was invalid: {error}"),
        )
    })
}

#[derive(Debug, Deserialize)]
struct FrankfurterRateRow {
    date: String,
    base: String,
    quote: String,
    rate: Number,
}

fn build_client() -> Client {
    Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .connect_timeout(std::time::Duration::from_secs(10))
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .expect("Frankfurter HTTP client should build")
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::NaiveDate;

    #[test]
    fn parses_usd_sek_fixture_into_rate_rows() {
        let rows = FrankfurterClient::parse_response(
            "USD",
            "SEK",
            include_str!("../../tests/fixtures/market_data/frankfurter_usd_sek.json"),
        )
        .expect("fixture should parse");

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].provider, FxProvider::Frankfurter);
        assert_eq!(rows[0].base, "USD");
        assert_eq!(rows[0].quote, "SEK");
        assert_eq!(
            rows[0].date,
            NaiveDate::from_ymd_opt(2026, 6, 10).expect("date should be valid")
        );
        assert_eq!(rows[0].rate.round_dp(4).to_string(), "9.4616");
    }

    #[test]
    fn parses_eur_sek_fixture_into_rate_rows() {
        let rows = FrankfurterClient::parse_response(
            "EUR",
            "SEK",
            include_str!("../../tests/fixtures/market_data/frankfurter_eur_sek.json"),
        )
        .expect("fixture should parse");

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[2].rate.round_dp(4).to_string(), "10.9037");
    }

    #[test]
    fn maps_http_errors_to_stable_reason_codes() {
        let error = ProviderError::with_http_status(
            FxProvider::Frankfurter.as_str(),
            reqwest::StatusCode::NOT_FOUND.as_u16(),
            "not found",
        );

        assert_eq!(error.reason(), ProviderMissingReason::NotListed);
        assert_eq!(error.reason_code(), "not_listed");
    }
}
