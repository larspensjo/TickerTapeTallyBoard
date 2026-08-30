use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{db::RepoError, providers::SymbolSearchMatch};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshMode {
    Latest,
    Backfill,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshTrigger {
    Manual,
    Launch,
    Backfill,
}

impl RefreshTrigger {
    pub(super) fn as_db_str(self) -> &'static str {
        match self {
            Self::Manual => "MANUAL",
            Self::Launch => "LAUNCH",
            Self::Backfill => "BACKFILL",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshRunStatus {
    Running,
    Succeeded,
    Partial,
    Failed,
}

impl RefreshRunStatus {
    pub(super) fn as_db_str(self) -> &'static str {
        match self {
            Self::Running => "RUNNING",
            Self::Succeeded => "SUCCEEDED",
            Self::Partial => "PARTIAL",
            Self::Failed => "FAILED",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshItemKind {
    Price,
    Fx,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshItemStatus {
    Fetched,
    Missing,
    /// The provider answered, but badly: a bad payload, a typed error envelope,
    /// or a currency that contradicts the instrument.
    Failed,
    Unmapped,
    /// A provider search returned more than one same-currency candidate. The
    /// instrument stays unmapped and needs a hand mapping.
    Ambiguous,
    /// The provider could not be reached at all. Distinct from `Failed` so a
    /// provider outage is explicit rather than a silent staleness slide.
    Unavailable,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RefreshPricesRequest {
    pub mode: RefreshMode,
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub end_date: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RefreshPricesResponse {
    pub run_id: i64,
    pub trigger: RefreshTrigger,
    pub mode: RefreshMode,
    pub status: RefreshRunStatus,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub message: Option<String>,
    pub prices_written: usize,
    pub fx_rates_written: usize,
    pub unmapped_instruments: usize,
    pub failed_items: usize,
    pub items: Vec<RefreshItem>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RefreshRunSummary {
    pub run_id: i64,
    pub trigger: RefreshTrigger,
    pub mode: RefreshMode,
    pub status: RefreshRunStatus,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub message: Option<String>,
    pub prices_written: usize,
    pub fx_rates_written: usize,
    pub unmapped_instruments: usize,
    pub failed_items: usize,
}

impl RefreshRunSummary {
    pub(super) fn running(
        run_id: i64,
        trigger: RefreshTrigger,
        mode: RefreshMode,
        started_at: String,
    ) -> Self {
        Self {
            run_id,
            trigger,
            mode,
            status: RefreshRunStatus::Running,
            started_at,
            finished_at: None,
            message: None,
            prices_written: 0,
            fx_rates_written: 0,
            unmapped_instruments: 0,
            failed_items: 0,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct RefreshItem {
    pub kind: RefreshItemKind,
    pub instrument_id: Option<i64>,
    /// The feed this item is about, so a failure names the provider that failed.
    pub provider: Option<String>,
    pub symbol_or_pair: String,
    pub status: RefreshItemStatus,
    pub reason: Option<String>,
    pub rows_written: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct PriceStatusResponse {
    pub refreshing: bool,
    pub latest_run: Option<RefreshRunSummary>,
    pub instruments: Vec<InstrumentMarketDataStatus>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct SymbolSearchLookupMatch {
    pub provider: String,
    pub provider_symbol: String,
    pub quote_type: Option<String>,
    pub exchange: Option<String>,
    pub name: Option<String>,
    pub asset_class: Option<String>,
    pub currency: Option<String>,
}

impl From<SymbolSearchMatch> for SymbolSearchLookupMatch {
    fn from(value: SymbolSearchMatch) -> Self {
        Self {
            provider: value.provider.to_string(),
            provider_symbol: value.provider_symbol,
            quote_type: value.quote_type,
            exchange: value.exchange,
            name: value.name,
            asset_class: value.asset_class,
            currency: value.currency,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SymbolSearchLookupStatus {
    Matches,
    NoMatch,
    ProviderUnavailable,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct SymbolSearchLookupResponse {
    pub query: String,
    pub status: SymbolSearchLookupStatus,
    pub matches: Vec<SymbolSearchLookupMatch>,
}

impl SymbolSearchLookupResponse {
    pub(crate) fn matches(query: String, matches: Vec<SymbolSearchLookupMatch>) -> Self {
        Self {
            query,
            status: SymbolSearchLookupStatus::Matches,
            matches,
        }
    }

    pub(crate) fn no_match(query: String) -> Self {
        Self {
            query,
            status: SymbolSearchLookupStatus::NoMatch,
            matches: Vec::new(),
        }
    }

    pub(crate) fn provider_unavailable(query: String) -> Self {
        Self {
            query,
            status: SymbolSearchLookupStatus::ProviderUnavailable,
            matches: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct InstrumentMarketDataStatus {
    pub instrument_id: i64,
    pub exchange: String,
    pub symbol: String,
    pub currency: String,
    /// Every mapping this instrument has, enabled or not, in precedence order.
    /// This replaces the single `mapping_enabled` / `provider_symbol` pair,
    /// which has no honest single-valued meaning once two sources exist.
    pub price_sources: Vec<PriceSourceStatus>,
    /// The provider code behind `latest_price`, or `None` when nothing resolved.
    pub effective_price_source: Option<String>,
    pub open_quantity: i64,
    pub latest_price: PriceSnapshotState,
    pub latest_fx: PriceSnapshotState,
}

#[derive(Clone, Debug, Serialize)]
pub struct PriceSourceStatus {
    pub provider: String,
    pub provider_symbol: String,
    pub asset_class: Option<String>,
    pub currency: Option<String>,
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotStatus {
    Available,
    Missing,
    Unmapped,
}

#[derive(Clone, Debug, Serialize)]
pub struct PriceSnapshotState {
    pub status: SnapshotStatus,
    pub date: Option<String>,
    pub value: Option<String>,
    pub provider: Option<String>,
    pub provider_symbol: Option<String>,
    pub reason: Option<String>,
}

impl PriceSnapshotState {
    pub(super) fn available(
        date: String,
        value: String,
        provider: String,
        provider_symbol: String,
    ) -> Self {
        Self {
            status: SnapshotStatus::Available,
            date: Some(date),
            value: Some(value),
            provider: Some(provider),
            provider_symbol: Some(provider_symbol),
            reason: None,
        }
    }

    pub(super) fn missing(reason: impl Into<String>) -> Self {
        Self {
            status: SnapshotStatus::Missing,
            date: None,
            value: None,
            provider: None,
            provider_symbol: None,
            reason: Some(reason.into()),
        }
    }

    pub(super) fn unmapped() -> Self {
        Self {
            status: SnapshotStatus::Unmapped,
            date: None,
            value: None,
            provider: None,
            provider_symbol: None,
            reason: Some("symbol_unmapped".to_owned()),
        }
    }
}

#[derive(Debug)]
pub enum MarketDataError {
    InvalidRequest { code: &'static str, message: String },
    Internal(String),
    Repo(RepoError),
}

impl MarketDataError {
    pub fn invalid_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::InvalidRequest {
            code,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }
}

impl fmt::Display for MarketDataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest { code, message } => write!(f, "{code}: {message}"),
            Self::Internal(message) => f.write_str(message),
            Self::Repo(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for MarketDataError {}

impl From<RepoError> for MarketDataError {
    fn from(error: RepoError) -> Self {
        Self::Repo(error)
    }
}

pub(super) fn refresh_status_from_db(status: &str) -> RefreshRunStatus {
    match status {
        "RUNNING" => RefreshRunStatus::Running,
        "SUCCEEDED" => RefreshRunStatus::Succeeded,
        "PARTIAL" => RefreshRunStatus::Partial,
        "FAILED" => RefreshRunStatus::Failed,
        _ => RefreshRunStatus::Failed,
    }
}

pub(super) fn refresh_trigger_from_db(trigger: &str) -> RefreshTrigger {
    match trigger {
        "LAUNCH" => RefreshTrigger::Launch,
        "BACKFILL" => RefreshTrigger::Backfill,
        _ => RefreshTrigger::Manual,
    }
}

pub(super) fn refresh_mode_from_trigger(trigger: &str) -> RefreshMode {
    match trigger {
        "BACKFILL" => RefreshMode::Backfill,
        _ => RefreshMode::Latest,
    }
}
