use super::*;
use crate::domain::LedgerTransaction;

pub(super) struct Loaded {
    pub figures: CompanyFigures,
    pub next_report: ReportDate,
    pub source: SourceState,
    pub link: Option<fundamentals_sources::SourceLink>,
    pub latest: Option<NaiveDate>,
    pub annual: Vec<NaiveDate>,
}

pub(super) fn local_date(
    clock: &crate::clock::Clock,
    id: i64,
    at: &str,
) -> Result<NaiveDate, ApiError> {
    let timestamp = chrono::DateTime::parse_from_rfc3339(at).map_err(|e| {
        ApiError::internal(format!(
            "invalid fundamentals timestamp instrument_id={id}: {e}"
        ))
    })?;
    Ok(clock.local_date(timestamp.into()))
}

pub(super) async fn load_inputs(
    pool: &SqlitePool,
    instrument: &instruments::InstrumentRow,
    today: NaiveDate,
    clock: &crate::clock::Clock,
    loaded_price: Option<&PriceSnapshot>,
    loaded_fx: Option<&FxSnapshot>,
    ledger: Option<&[LedgerTransaction]>,
) -> Result<Loaded, ApiError> {
    let id = instrument.id;
    let stored = fundamental_values::active_values(pool, id).await?;
    let (link, state) = if ledger.is_some() {
        (None, fundamentals_sources::figures_state(pool, id).await?)
    } else {
        let link = fundamentals_sources::find(pool, id).await?;
        let state = link.as_ref().map(fundamentals_sources::FiguresState::from);
        (link, state)
    };
    let awaiting = stored.is_empty()
        && !state
            .as_ref()
            .is_some_and(|s| s.status == LinkStatus::NotApplicable)
        && state.as_ref().is_none_or(|s| {
            s.last_attempt_status.is_none() || s.last_attempt_status == Some(AttemptStatus::Failed)
        })
        && fundamental_values::has_set_aside_values(pool, id).await?;
    let mut source = SourceState {
        classification: state
            .as_ref()
            .map_or(Classification::Unknown, |s| s.classification),
        status: state.as_ref().map_or(LinkStatus::Pending, |s| s.status),
        awaiting_figures: awaiting,
        unreachable: state
            .as_ref()
            .is_some_and(|s| s.last_attempt_status == Some(AttemptStatus::Failed)),
        last_success: state
            .as_ref()
            .and_then(|s| s.last_success_at.as_deref())
            .map(|at| local_date(clock, id, at))
            .transpose()?,
    };
    if effective_prices::enabled_price_sources(pool, id)
        .await?
        .iter()
        .any(|s| s.asset_class.as_deref() == Some("TRACKER_CERTIFICATES"))
    {
        source.classification = Classification::Certificate;
    }
    let values = stored
        .into_iter()
        .map(|v| {
            let fetched_on = local_date(clock, id, &v.updated_at)?;
            Ok(ReportedValue {
                input: v.input,
                fetched_on,
            })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
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
    let history = fundamental_values::active_report_history(pool, id).await?;
    let next_report = report_dates::next_report(today, latest, &annual, &history, &source);
    let price = loaded_price.map(|p| Price {
        date: p.date,
        close: p.close,
        currency: p.currency.clone(),
        stale: matches!(p.freshness, DataFreshness::WarningStale { .. }),
    });
    let evidence_date = values
        .iter()
        .filter(|v| v.input.metric == Metric::MarketCap)
        .map(|v| v.input.period_end)
        .max();
    let evidence_price = match evidence_date {
        Some(d) if price.as_ref().is_some_and(|p| p.date <= d) => price.clone(),
        Some(d) => effective_prices::effective_latest_on_or_before(pool, id, d)
            .await?
            .map(|p| Price {
                date: p.date,
                close: p.close,
                currency: p.currency,
                stale: false,
            }),
        None => None,
    };
    // Read FX only for crossings used by the ratios or independent unit evidence.
    // Same-currency figures require no FX, and each (currency, date) is read once.
    let mut requests = std::collections::BTreeSet::new();
    if let Some(price) = &price {
        for currency in values
            .iter()
            .filter(|v| {
                matches!(
                    v.input.period_type,
                    PeriodType::Quarter | PeriodType::Annual
                )
            })
            .filter(|v| {
                latest.is_some_and(|end| {
                    v.input.period_end <= end && (end - v.input.period_end).num_days() <= 300
                })
            })
            .filter_map(|v| v.input.currency.as_deref())
            .filter(|c| *c != price.currency)
        {
            requests.insert((currency.to_owned(), price.date));
            requests.insert((price.currency.clone(), price.date));
        }
    }
    if let (Some(evidence), Some(price)) = (
        values
            .iter()
            .filter(|v| v.input.metric == Metric::MarketCap)
            .max_by_key(|v| v.input.period_end),
        &evidence_price,
    ) {
        if let Some(currency) = evidence
            .input
            .currency
            .as_deref()
            .filter(|c| *c != price.currency)
        {
            requests.insert((currency.to_owned(), evidence.input.period_end));
            requests.insert((price.currency.clone(), evidence.input.period_end));
        }
    }
    let mut fx = Vec::new();
    for (currency, date) in requests
        .into_iter()
        .filter(|(c, _)| c != "SEK" && company_figures::ECB_CURRENCIES.contains(&c.as_str()))
    {
        if let Some(f) = loaded_fx.filter(|f| {
            f.base == currency
                && f.quote == "SEK"
                && f.date <= date
                && price.as_ref().is_some_and(|p| p.date == date)
        }) {
            fx.push(FxRate {
                currency,
                date: f.date,
                sek_per_unit: f.rate,
            });
        } else if let Some(f) =
            fx_rates::find_latest_on_or_before(pool, &currency, "SEK", BASE_FX_PROVIDER, date)
                .await?
        {
            fx.push(FxRate {
                currency,
                date: f.date_value()?,
                sek_per_unit: f.rate_decimal()?,
            });
        }
    }
    let mut splits: Vec<_> = fundamental_values::active_splits(pool, id)
        .await?
        .iter()
        .map(|s| s.date)
        .collect();
    if let Some(ledger) = ledger {
        splits.extend(
            ledger
                .iter()
                .filter(|t| t.kind == TransactionKind::Split)
                .map(|t| t.trade_date),
        );
    } else {
        splits.extend(
            transactions::ledger_for_instrument(pool, id)
                .await?
                .iter()
                .filter(|t| t.kind == TransactionKind::Split)
                .map(|t| t.trade_date),
        );
    }
    let compute = if ledger.is_some() {
        company_figures::compute_holdings
    } else {
        company_figures::compute
    };
    let figures = compute(&Inputs {
        today,
        values: &values,
        splits: &splits,
        price: price.as_ref(),
        evidence_price: evidence_price.as_ref(),
        fx: &fx,
        source: &source,
        next_report: next_report.expectation(),
    });
    Ok(Loaded {
        figures,
        next_report,
        source,
        link,
        latest,
        annual,
    })
}
