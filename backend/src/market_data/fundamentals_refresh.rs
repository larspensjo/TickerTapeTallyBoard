use super::{
    fundamentals_source_discovery::{self as discovery, Discovery},
    provider_registry::ProviderSet,
    refresh::{RefreshLease, RefreshTarget},
    refresh_contract::MarketDataError,
};
use crate::domain::fundamentals::FundamentalsStatus;
use crate::{
    db::{
        fundamental_values::{self, FetchWrite},
        fundamentals_sources::{self, AttemptStatus, GenerationWrite, LinkOrigin, LinkStatus},
        market_data_runs,
    },
    domain::{
        fundamentals::PeriodType,
        report_dates::{self, FetchDecision, FetchTiming},
    },
    providers::{
        yahoo_fundamentals::{FundamentalsRequest, CHART_ENDPOINT, TIMESERIES_ENDPOINT},
        ProviderMissingReason,
    },
};
use chrono::{DateTime, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::atomic::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FundamentalsItemStatus {
    Fetched,
    Skipped,
    Superseded,
    Failed,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FundamentalsItem {
    pub instrument_id: i64,
    pub symbol: Option<String>,
    pub status: FundamentalsItemStatus,
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fx_pair: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FundamentalsSummary {
    pub status: FundamentalsStatus,
    // Counts describe instruments; report-currency FX outcomes live in items.
    pub fetched: usize,
    pub skipped: usize,
    pub superseded: usize,
    pub failed: usize,
    pub items: Vec<FundamentalsItem>,
}
impl FundamentalsSummary {
    pub(super) fn push(&mut self, item: FundamentalsItem) {
        if item.fx_pair.is_none() {
            match item.status {
                FundamentalsItemStatus::Fetched => self.fetched += 1,
                FundamentalsItemStatus::Skipped => self.skipped += 1,
                FundamentalsItemStatus::Superseded => self.superseded += 1,
                FundamentalsItemStatus::Failed => self.failed += 1,
            }
        }
        self.items.push(item);
        self.status = if !self
            .items
            .iter()
            .any(|item| item.status == FundamentalsItemStatus::Failed)
        {
            FundamentalsStatus::Succeeded
        } else if !self
            .items
            .iter()
            .any(|item| item.status == FundamentalsItemStatus::Fetched)
        {
            FundamentalsStatus::Failed
        } else {
            FundamentalsStatus::Partial
        };
    }
}

pub(super) async fn run(
    providers: &ProviderSet,
    pool: &SqlitePool,
    targets: &[RefreshTarget],
    clock: &crate::clock::Clock,
    window: &super::refresh::RefreshWindow,
    lease: &RefreshLease,
    mut attempted_fx: std::collections::BTreeSet<String>,
) -> Result<FundamentalsSummary, MarketDataError> {
    let mut summary = FundamentalsSummary::default();
    let mut fx_failed = merge_fx(
        &mut summary,
        lease,
        super::fx_refresh::complete_report_fx(
            providers,
            pool,
            targets,
            window,
            lease,
            &mut attempted_fx,
        )
        .await,
    )?;
    super::refresh::MarketDataService::publish_prices(pool, lease).await?;
    match refresh(providers, pool, targets, clock, lease).await {
        Ok(result) => {
            let fx_items = std::mem::take(&mut summary.items);
            summary = result;
            for item in fx_items {
                summary.push(item);
            }
        }
        Err(MarketDataError::LeaseLost) => return Err(MarketDataError::LeaseLost),
        Err(error) => record_failure(&mut summary, lease, &error),
    }
    fx_failed |= merge_fx(
        &mut summary,
        lease,
        super::fx_refresh::complete_report_fx(
            providers,
            pool,
            targets,
            window,
            lease,
            &mut attempted_fx,
        )
        .await,
    )?;
    if fx_failed {
        summary.status = if summary.fetched == 0 {
            FundamentalsStatus::Failed
        } else {
            FundamentalsStatus::Partial
        };
    }
    let stored = summary.clone();
    let run_id = lease.run_id;
    let written = market_data_runs::fenced_write(pool, run_id, &lease.owner, |conn| {
        Box::pin(async move {
            market_data_runs::store_fundamentals_summary(
                conn,
                run_id,
                stored.status,
                market_data_runs::FundamentalsCounts {
                    fetched: stored.fetched as i64,
                    skipped: stored.skipped as i64,
                    superseded: stored.superseded as i64,
                    failed: stored.failed as i64,
                },
            )
            .await
        })
    })
    .await?;
    if written.is_err() {
        return Err(MarketDataError::LeaseLost);
    }
    Ok(summary)
}
fn record_failure(
    summary: &mut FundamentalsSummary,
    lease: &RefreshLease,
    error: &MarketDataError,
) {
    crate::engine_error!("fundamentals operation failure instrument_id=none symbol=none endpoint=repository run_id={} link_generation=none reason=repository_error message={}", lease.run_id, error);
    summary.status = if summary.fetched == 0 {
        FundamentalsStatus::Failed
    } else {
        FundamentalsStatus::Partial
    };
}
fn merge_fx(
    summary: &mut FundamentalsSummary,
    lease: &RefreshLease,
    outcome: Result<Vec<FundamentalsItem>, MarketDataError>,
) -> Result<bool, MarketDataError> {
    match outcome {
        Ok(items) => {
            for item in items {
                summary.push(item);
            }
        }
        Err(MarketDataError::LeaseLost) => return Err(MarketDataError::LeaseLost),
        Err(error) => {
            record_failure(summary, lease, &error);
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn local_day(
    clock: &crate::clock::Clock,
    at: Option<&str>,
) -> Result<Option<NaiveDate>, MarketDataError> {
    at.map(|s| {
        DateTime::parse_from_rfc3339(s)
            .map(|t| clock.local_date(t.to_utc()))
            .map_err(|e| MarketDataError::internal(format!("bad fundamentals timestamp {s}: {e}")))
    })
    .transpose()
}

pub(super) async fn refresh(
    providers: &ProviderSet,
    pool: &SqlitePool,
    targets: &[RefreshTarget],
    clock: &crate::clock::Clock,
    lease: &RefreshLease,
) -> Result<FundamentalsSummary, MarketDataError> {
    let mut summary = FundamentalsSummary::default();
    if providers.fundamentals_provider().is_none() {
        return Ok(summary);
    }
    summary.status = FundamentalsStatus::Succeeded;
    let mut limited = false;
    for target in targets {
        if lease.cancelled.load(Ordering::Acquire) {
            return Err(MarketDataError::LeaseLost);
        }
        let today = clock.today();
        let id = target.instrument.id;
        let mut item = FundamentalsItem {
            instrument_id: id,
            symbol: Some(target.instrument.symbol.clone()),
            status: FundamentalsItemStatus::Skipped,
            reason: None,
            fx_pair: None,
        };
        let mut generation = 0;
        let result: Result<(), MarketDataError> = async {
            let link = fundamentals_sources::find(pool, id).await?;
            let expected = link.as_ref().map(|s| s.token.clone());
            if let Some(symbol) = expected.as_ref().and_then(|t| t.symbol.clone()) {
                item.symbol = Some(symbol);
            }
            generation = expected.as_ref().map_or(0, |t| t.generation);
            if limited {
                item.reason = Some("skipped_rate_limited".to_owned());
                return Ok(());
            }
            if link
                .as_ref()
                .is_some_and(|s| s.definition.status == LinkStatus::NotApplicable)
            {
                item.reason = Some(
                    match link.as_ref().expect("link").definition.classification {
                        fundamentals_sources::Classification::Fund => "fund_or_etf",
                        fundamentals_sources::Classification::Certificate => "certificate",
                        _ => "not_a_company",
                    }
                    .to_owned(),
                );
                return Ok(());
            }
            let values = fundamental_values::active_values(pool, id).await?;
            let observations = fundamental_values::active_observations(pool, id).await?;
            let latest = values
                .iter()
                .filter(|v| v.input.period_type == PeriodType::Quarter)
                .map(|v| v.input.period_end)
                .max();
            let annual: Vec<_> = values
                .iter()
                .filter(|v| v.input.period_type == PeriodType::Annual)
                .map(|v| v.input.period_end)
                .collect();
            let real: Vec<_> = observations
                .iter()
                .filter_map(|o| {
                    report_dates::observed_report(o.first_seen_on, o.absent_fetch_on)
                        .map(|r| (o.period_end, r))
                })
                .collect();
            let estimate = report_dates::app_estimate(latest, &annual, &real);
            let timing = FetchTiming {
                last_attempt: local_day(
                    clock,
                    link.as_ref().and_then(|s| s.last_attempt_at.as_deref()),
                )?,
                last_success: local_day(
                    clock,
                    link.as_ref().and_then(|s| s.last_success_at.as_deref()),
                )?,
                previous_failed: link.as_ref().is_some_and(|s| {
                    s.last_attempt_status == Some(AttemptStatus::Failed)
                        && matches!(
                            s.last_attempt_reason.as_deref(),
                            Some("rate_limited" | "provider_unavailable")
                        )
                }),
                next_report: estimate.map(|e| e.date),
                newer_quarter_seen_on: None,
            };
            if report_dates::fetch_decision(today, &timing) == FetchDecision::NotDue {
                item.reason = Some("not_due".to_owned());
                return Ok(());
            }
            let rediscover = link.as_ref().is_none_or(|s| {
                s.definition.origin == LinkOrigin::Discovered
                    && (matches!(
                        s.definition.status,
                        LinkStatus::Pending | LinkStatus::NoMatch | LinkStatus::Ambiguous
                    ) || s.consecutive_empty >= 3
                        || s.last_attempt_reason.as_deref() == Some("not_listed"))
            });
            let result = if rediscover {
                discovery::discover(
                    providers,
                    pool,
                    &target.instrument,
                    today,
                    lease,
                    generation,
                )
                .await
            } else {
                fetch_link(
                    providers,
                    id,
                    link.as_ref().expect("existing link"),
                    today,
                    lease,
                )
                .await
            };
            let mut write = FetchWrite {
                instrument_id: id,
                expected,
                discovery: None,
                verified: None,
                values: Vec::new(),
                splits: Vec::new(),
                at: clock.now_utc().to_rfc3339(),
                seen_on: clock.today(),
                previous_success_on: timing.last_success,
                status: AttemptStatus::Failed,
                reason: None,
            };
            match result {
                Ok(found) => {
                    item.symbol = found.definition.symbol.clone();
                    let applicable = found.definition.status == LinkStatus::NotApplicable;
                    write.status = if !discovery::has_quarters(&found.values) {
                        AttemptStatus::Empty
                    } else {
                        AttemptStatus::Succeeded
                    };
                    item.status = if applicable {
                        FundamentalsItemStatus::Skipped
                    } else if discovery::has_quarters(&found.values) {
                        FundamentalsItemStatus::Fetched
                    } else {
                        FundamentalsItemStatus::Failed
                    };
                    item.reason = found.reason.clone();
                    write.reason = found.reason;
                    write.values = found.values;
                    write.splits = found.splits;
                    if rediscover {
                        write.discovery = Some(found.definition);
                    } else {
                        write.verified = Some(found.definition);
                    }
                }
                Err(MarketDataError::FundamentalsProvider(error)) => {
                    limited = error.reason() == ProviderMissingReason::RateLimited;
                    item.status = FundamentalsItemStatus::Failed;
                    item.reason = Some(error.reason_code().to_owned());
                    write.reason = item.reason.clone();
                    if link.is_none() {
                        write.discovery = Some(fundamentals_sources::LinkDefinition::pending());
                    }
                }
                Err(error) => return Err(error),
            }
            if lease.cancelled.load(Ordering::Acquire) {
                return Err(MarketDataError::LeaseLost);
            }
            let result = market_data_runs::fenced_write(pool, lease.run_id, &lease.owner, |conn| {
                Box::pin(async move { fundamental_values::write_fetch(conn, &write).await })
            })
            .await?;
            match result {
                Err(_) => return Err(MarketDataError::LeaseLost),
                Ok(GenerationWrite::Superseded) => {
                    item.status = FundamentalsItemStatus::Superseded;
                    item.reason = Some("superseded".to_owned());
                }
                Ok(GenerationWrite::Applied(_)) => {}
            }
            Ok(())
        }
        .await;
        if lease.cancelled.load(Ordering::Acquire) {
            return Err(MarketDataError::LeaseLost);
        }
        match result {
            Ok(()) => {}
            Err(MarketDataError::LeaseLost) => return Err(MarketDataError::LeaseLost),
            Err(error) => {
                crate::engine_error!("fundamentals operation failure instrument_id={} symbol={:?} endpoint=repository run_id={} link_generation={} reason=repository_error message={}", id, item.symbol, lease.run_id, generation, error);
                item.status = FundamentalsItemStatus::Failed;
                item.reason = Some("repository_error".to_owned());
            }
        }
        crate::engine_info!("fundamentals result instrument_id={} symbol={:?} endpoint={} run_id={} link_generation={} status={:?} reason={:?}", id, item.symbol, TIMESERIES_ENDPOINT, lease.run_id, generation, item.status, item.reason);
        summary.push(item);
    }
    Ok(summary)
}

async fn fetch_link(
    providers: &ProviderSet,
    instrument_id: i64,
    link: &fundamentals_sources::SourceLink,
    today: NaiveDate,
    lease: &RefreshLease,
) -> Result<Discovery, MarketDataError> {
    let symbol = link
        .definition
        .symbol
        .as_deref()
        .ok_or_else(|| MarketDataError::internal("fetchable link has no symbol"))?;
    let request = FundamentalsRequest {
        symbol: symbol.to_owned(),
        end: today,
    };
    let client = providers.fundamentals_provider().expect("registered");
    let values = client.fetch(&request).await.map_err(|error| {
        discovery::log_failure(
            instrument_id,
            symbol,
            lease,
            link.token.generation,
            TIMESERIES_ENDPOINT,
            &error,
        );
        discovery::provider_error(error)
    })?;
    let listing = client.listing(&request).await.map_err(|error| {
        discovery::log_failure(
            instrument_id,
            symbol,
            lease,
            link.token.generation,
            CHART_ENDPOINT,
            &error,
        );
        discovery::provider_error(error)
    })?;
    let mut definition = link.definition.clone();
    if listing.security_type.is_some()
        && (definition.security_type.is_none()
            || discovery::classify(listing.security_type.as_deref())
                != fundamentals_sources::Classification::Unknown)
    {
        definition.security_type = listing.security_type;
        definition.classification = discovery::classify(definition.security_type.as_deref());
    }
    let has_quarters = discovery::has_quarters(&values);
    if has_quarters && definition.classification == fundamentals_sources::Classification::Unknown {
        definition.classification = fundamentals_sources::Classification::Figures;
    }
    definition.status = if matches!(
        definition.classification,
        fundamentals_sources::Classification::Fund
            | fundamentals_sources::Classification::Certificate
    ) {
        LinkStatus::NotApplicable
    } else if has_quarters {
        LinkStatus::Usable
    } else if definition.status == LinkStatus::Pending {
        LinkStatus::Unavailable
    } else {
        definition.status
    };
    let reason = (!has_quarters).then(|| {
        if definition.classification == fundamentals_sources::Classification::Unknown {
            "security_type_unknown"
        } else {
            "no_quarterly_data"
        }
        .to_owned()
    });
    Ok(Discovery {
        definition,
        values,
        splits: listing.splits,
        reason,
    })
}
