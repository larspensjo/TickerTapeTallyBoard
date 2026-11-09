//! Provider calls stay outside transactions. All plans are re-derived from stored
//! announcements inside the lease fence, after checking the current US identity.
use super::{
    fundamentals_refresh::{FundamentalsItem, FundamentalsItemStatus},
    provider_registry::ProviderSet,
    refresh::{RefreshLease, RefreshTarget},
    refresh_contract::MarketDataError,
};
use crate::{
    db::{
        market_data_runs,
        report_announcements::{self as repo, UsTicker},
    },
    domain::report_dates::*,
    providers::{
        nasdaq_earnings_calendar::{calendar_url, symbol_url},
        ProviderError,
    },
};
use chrono::NaiveDate;
use futures::{stream, StreamExt};
use sqlx::SqlitePool;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Clone)]
struct Target {
    id: i64,
    ticker: UsTicker,
}

/// Shared by lookups before figures and fallback checks afterwards, so a rate
/// limit stops new Nasdaq requests throughout the entire refresh run.
#[derive(Default)]
pub(super) struct CalendarRun {
    pub attempted_lookups: BTreeSet<i64>,
    limited: AtomicBool,
}
fn check(lease: &RefreshLease) -> Result<(), MarketDataError> {
    if lease.cancelled.load(Ordering::Acquire) {
        Err(MarketDataError::LeaseLost)
    } else {
        Ok(())
    }
}
fn item(target: &Target, status: FundamentalsItemStatus, reason: &str) -> FundamentalsItem {
    FundamentalsItem {
        instrument_id: target.id,
        symbol: Some(target.ticker.symbol.clone()),
        status,
        reason: Some(reason.into()),
        fx_pair: None,
    }
}
fn log_failure(target: &Target, lease: &RefreshLease, url: &str, error: &ProviderError) {
    crate::engine_error!("report date provider failure instrument_id={} symbol={} URL={} run_id={} link_generation={} reason={} message={}", target.id, target.ticker.symbol, url, lease.run_id, target.ticker.link_generation.map_or_else(|| "none".into(), |g| g.to_string()), error.reason_code(), error);
}
async fn targets(
    pool: &SqlitePool,
    targets: &[RefreshTarget],
) -> Result<Vec<Target>, MarketDataError> {
    let mut result = vec![];
    for target in targets {
        if let Some(ticker) = repo::ticker(pool, target.instrument.id).await? {
            result.push(Target {
                id: target.instrument.id,
                ticker,
            });
        }
    }
    Ok(result)
}

/// Resolve both before the calendar fallback (so arrived quarters cannot be
/// withdrawn) and after the figures fetch (so same-run quarters resolve dates).
pub(super) async fn resolve(
    pool: &SqlitePool,
    targets_in: &[RefreshTarget],
    today: NaiveDate,
    lease: &RefreshLease,
) -> Result<Vec<FundamentalsItem>, MarketDataError> {
    let mut outcomes = vec![];
    for input in targets_in {
        check(lease)?;
        let expected = repo::ticker(pool, input.instrument.id).await?;
        let Some(ticker) = expected else {
            let id = input.instrument.id;
            let written =
                market_data_runs::fenced_write(pool, lease.run_id, &lease.owner, |conn| {
                    Box::pin(async move {
                        if repo::current_ticker(conn, id).await?.is_none() {
                            repo::supersede_other_symbols(conn, id, None).await?;
                        }
                        Ok(())
                    })
                })
                .await?;
            if written.is_err() {
                return Err(MarketDataError::LeaseLost);
            }
            continue;
        };
        let target = Target {
            id: input.instrument.id,
            ticker,
        };
        let stored = target.clone();
        let written = market_data_runs::fenced_write(pool, lease.run_id, &lease.owner, |conn| {
            Box::pin(async move {
                let target = stored;
                if repo::current_ticker(conn, target.id).await?.as_ref() != Some(&target.ticker) {
                    return Ok(false);
                }
                repo::supersede_other_symbols(conn, target.id, Some(&target.ticker)).await?;
                let entries = repo::for_ticker(&mut *conn, target.id, &target.ticker).await?;
                let observations = repo::observations(&mut *conn, target.id).await?;
                let changes = resolve_announcements(&entries, &observations);
                repo::apply(conn, target.id, &target.ticker, today, &changes).await?;
                Ok(true)
            })
        })
        .await?;
        if written.is_err() {
            return Err(MarketDataError::LeaseLost);
        }
        if written == Ok(false) {
            outcomes.push(item(
                &target,
                FundamentalsItemStatus::Superseded,
                "superseded",
            ));
        }
    }
    Ok(outcomes)
}

pub(super) async fn refresh(
    providers: &ProviderSet,
    pool: &SqlitePool,
    targets_in: &[RefreshTarget],
    clock: &crate::clock::Clock,
    lease: &RefreshLease,
    include_presence_checks: bool,
    run: &CalendarRun,
) -> Result<Vec<FundamentalsItem>, MarketDataError> {
    let limited = &run.limited;
    let Some(provider) = providers.earnings_calendar_provider() else {
        return Ok(vec![]);
    };
    let today = clock.today();
    let mut outcomes: BTreeMap<_, _> = resolve(pool, targets_in, today, lease)
        .await?
        .into_iter()
        .map(|i| (i.instrument_id, i))
        .collect();
    let targets = targets(pool, targets_in).await?;
    let mut jobs = vec![];
    for target in &targets {
        check(lease)?;
        if run.attempted_lookups.contains(&target.id) {
            continue;
        }
        let entries = repo::for_ticker(pool, target.id, &target.ticker).await?;
        if lookup_due(
            today,
            &repo::lookup(pool, target.id, &target.ticker).await?,
            &entries,
        ) {
            jobs.push(target.clone());
        }
    }
    let responses: Vec<_> = stream::iter(jobs)
        .map(|target| async move {
            check(lease)?;
            if limited.load(Ordering::Acquire) {
                return Ok((target, None));
            }
            let response = provider.report_date(&target.ticker.symbol).await;
            if response
                .as_ref()
                .is_err_and(|e| e.reason() == crate::providers::ProviderMissingReason::RateLimited)
            {
                limited.store(true, Ordering::Release);
            }
            Ok::<_, MarketDataError>((target, Some(response)))
        })
        .buffer_unordered(4)
        .collect()
        .await;
    let mut answers = BTreeMap::new();
    for response in responses {
        check(lease)?;
        let (target, response) = response?;
        let Some(response) = response else {
            outcomes.insert(
                target.id,
                item(
                    &target,
                    FundamentalsItemStatus::Skipped,
                    "skipped_rate_limited",
                ),
            );
            continue;
        };
        let (outcome, answer) = match response {
            Ok(answer) => {
                let outcome = answer.outcome();
                (outcome, Some(answer))
            }
            Err(error) => {
                log_failure(&target, lease, &symbol_url(&target.ticker.symbol), &error);
                outcomes.insert(
                    target.id,
                    item(
                        &target,
                        FundamentalsItemStatus::Failed,
                        &format!("nasdaq_{}", error.reason_code()),
                    ),
                );
                (LookupOutcome::Failed, None)
            }
        };
        let id = target.id;
        answers.insert(id, answer.clone());
        let stored = target.clone();
        let written = market_data_runs::fenced_write(pool, lease.run_id, &lease.owner, |conn| {
            Box::pin(async move {
                if repo::current_ticker(conn, stored.id).await?.as_ref() != Some(&stored.ticker) {
                    return Ok(false);
                }
                repo::supersede_other_symbols(conn, stored.id, Some(&stored.ticker)).await?;
                if let Some(SymbolReportDate::Dated(report)) = answer {
                    let entries = repo::for_ticker(&mut *conn, stored.id, &stored.ticker).await?;
                    let baseline = repo::observations(&mut *conn, stored.id)
                        .await?
                        .iter()
                        .map(|o| o.period_end)
                        .max();
                    let changes = reconcile_date(&entries, &report, false, today, baseline);
                    repo::apply(conn, stored.id, &stored.ticker, today, &changes).await?;
                }
                repo::record_lookup(conn, stored.id, &stored.ticker, today, outcome).await?;
                Ok(true)
            })
        })
        .await?;
        match written {
            Err(_) => return Err(MarketDataError::LeaseLost),
            Ok(false) => {
                outcomes.insert(
                    id,
                    item(&target, FundamentalsItemStatus::Superseded, "superseded"),
                );
            }
            Ok(true) => {
                outcomes.entry(id).or_insert_with(|| {
                    item(
                        &target,
                        FundamentalsItemStatus::Fetched,
                        "report_date_lookup",
                    )
                });
            }
        }
    }
    for outcome in resolve(pool, targets_in, today, lease).await? {
        outcomes.insert(outcome.instrument_id, outcome);
    }
    if !include_presence_checks {
        return Ok(outcomes.into_values().collect());
    }
    let scanned = repo::scanned_today(pool, today).await?;
    let mut days = BTreeSet::new();
    for target in &targets {
        check(lease)?;
        // Identity may have changed during the bounded provider calls.
        if repo::ticker(pool, target.id).await?.as_ref() != Some(&target.ticker) {
            continue;
        }
        let entries = repo::for_ticker(pool, target.id, &target.ticker).await?;
        let state = repo::lookup(pool, target.id, &target.ticker).await?;
        let latest_answer = match state.last_outcome {
            Some(LookupOutcome::None) => Some(SymbolReportDate::None),
            Some(LookupOutcome::UnknownSymbol) => Some(SymbolReportDate::UnknownSymbol),
            _ => None,
        };
        days.extend(scan_days(
            today,
            &entries,
            answers
                .get(&target.id)
                .and_then(Option::as_ref)
                .or(latest_answer.as_ref()),
            &scanned,
        ));
    }
    for day in days {
        check(lease)?;
        let mut interested = vec![];
        for target in &targets {
            check(lease)?;
            if repo::for_ticker(pool, target.id, &target.ticker)
                .await?
                .iter()
                .any(|a| a.state == AnnouncementState::Active && a.report.date == day)
            {
                interested.push(target.clone());
            }
        }
        if interested.is_empty() {
            continue;
        }
        if limited.load(Ordering::Acquire) {
            for target in &interested {
                let existing = outcomes.entry(target.id).or_insert_with(|| {
                    item(
                        target,
                        FundamentalsItemStatus::Skipped,
                        "skipped_rate_limited",
                    )
                });
                if existing.status == FundamentalsItemStatus::Fetched {
                    *existing = item(
                        target,
                        FundamentalsItemStatus::Skipped,
                        "skipped_rate_limited",
                    );
                }
            }
            continue;
        }
        let sightings = match provider.calendar(day, today).await {
            Ok(sightings) => sightings,
            Err(error) => {
                if error.reason() == crate::providers::ProviderMissingReason::RateLimited {
                    limited.store(true, Ordering::Release);
                }
                for target in &interested {
                    check(lease)?;
                    log_failure(target, lease, &calendar_url(day), &error);
                    outcomes.insert(
                        target.id,
                        item(
                            target,
                            FundamentalsItemStatus::Failed,
                            &format!("nasdaq_calendar_{}", error.reason_code()),
                        ),
                    );
                }
                let stored_targets = interested.clone();
                let cancelled = lease.cancelled.clone();
                let at = clock.now_utc().to_rfc3339();
                let written =
                    market_data_runs::fenced_write(pool, lease.run_id, &lease.owner, |conn| {
                        Box::pin(async move {
                            let mut matched = false;
                            let mut superseded = vec![];
                            for target in stored_targets {
                                if cancelled.load(Ordering::Acquire) {
                                    return Err(crate::db::RepoError::Decode(
                                        "report calendar lease cancelled".into(),
                                    ));
                                }
                                if repo::current_ticker(conn, target.id).await?.as_ref()
                                    != Some(&target.ticker)
                                {
                                    superseded.push(target.id);
                                    continue;
                                }
                                matched = true;
                            }
                            if matched {
                                repo::record_scan(
                                    conn,
                                    day,
                                    today,
                                    &at,
                                    crate::domain::fundamentals::AttemptStatus::Failed,
                                )
                                .await?;
                            }
                            Ok(superseded)
                        })
                    })
                    .await;
                check(lease)?;
                match written? {
                    Err(_) => return Err(MarketDataError::LeaseLost),
                    Ok(ids) => {
                        for id in ids {
                            if let Some(target) = interested.iter().find(|t| t.id == id) {
                                outcomes.insert(
                                    id,
                                    item(target, FundamentalsItemStatus::Superseded, "superseded"),
                                );
                            }
                        }
                    }
                }
                continue;
            }
        };
        check(lease)?;
        let stored_targets = interested;
        let cancelled = lease.cancelled.clone();
        let at = clock.now_utc().to_rfc3339();
        let written = market_data_runs::fenced_write(pool, lease.run_id, &lease.owner, |conn| {
            Box::pin(async move {
                if repo::scanned_today(&mut *conn, today).await?.contains(&day) {
                    return Ok(vec![]);
                }
                let mut superseded = vec![];
                let mut matched = false;
                for target in stored_targets {
                    if cancelled.load(Ordering::Acquire) {
                        return Err(crate::db::RepoError::Decode(
                            "report calendar lease cancelled".into(),
                        ));
                    }
                    if repo::current_ticker(conn, target.id).await?.as_ref() != Some(&target.ticker)
                    {
                        superseded.push(target.id);
                        continue;
                    }
                    matched = true;
                    repo::supersede_other_symbols(conn, target.id, Some(&target.ticker)).await?;
                    let entries = repo::for_ticker(&mut *conn, target.id, &target.ticker).await?;
                    // A fallback scans only the stored announcement's day; unrelated
                    // held symbols in that response do not create announcements.
                    if !entries
                        .iter()
                        .any(|a| a.state == AnnouncementState::Active && a.report.date == day)
                    {
                        continue;
                    }
                    let absent =
                        reconcile_absence(&entries, day, &sightings, &target.ticker.symbol);
                    repo::apply(conn, target.id, &target.ticker, today, &absent).await?;
                    for sighting in sightings
                        .iter()
                        .filter(|s| s.symbol.eq_ignore_ascii_case(&target.ticker.symbol))
                    {
                        let entries =
                            repo::for_ticker(&mut *conn, target.id, &target.ticker).await?;
                        let baseline = repo::observations(&mut *conn, target.id)
                            .await?
                            .iter()
                            .map(|o| o.period_end)
                            .max();
                        let changes = reconcile_date(
                            &entries,
                            &sighting.report,
                            sighting.presence_only,
                            today,
                            baseline,
                        );
                        repo::apply(conn, target.id, &target.ticker, today, &changes).await?;
                    }
                }
                if matched {
                    repo::record_scan(
                        conn,
                        day,
                        today,
                        &at,
                        crate::domain::fundamentals::AttemptStatus::Succeeded,
                    )
                    .await?;
                }
                Ok(superseded)
            })
        })
        .await;
        check(lease)?;
        let written = written?;
        match written {
            Err(_) => return Err(MarketDataError::LeaseLost),
            Ok(ids) => {
                for id in ids {
                    if let Some(target) = targets.iter().find(|t| t.id == id) {
                        outcomes.insert(
                            id,
                            item(target, FundamentalsItemStatus::Superseded, "superseded"),
                        );
                    }
                }
            }
        }
    }
    Ok(outcomes.into_values().collect())
}
