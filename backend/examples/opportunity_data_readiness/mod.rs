mod business;
mod history;
mod reports;
#[cfg(test)]
mod tests;

use chrono::Datelike;
use sqlx::SqlitePool;
use std::collections::{BTreeMap, BTreeSet};
use ticker_tape_tally_board_backend::{
    api::{load_company_figure_inputs, CompanyFigureScope},
    clock::Clock,
    db::{self, instruments, prices, transactions},
    domain::{
        derive_position, recent_price_change, value_position, RecentPriceChange, RecentWindow,
        TransactionKind,
    },
    ledger_analysis::aggregate_share,
    market_data::effective_prices,
};

type Error = Box<dyn std::error::Error + Send + Sync>;
pub const WINDOWS: [RecentWindow; 3] = [
    RecentWindow::OneWeek,
    RecentWindow::TwoWeeks,
    RecentWindow::ThirtyDays,
];

#[derive(Default)]
pub struct Summary {
    pub tables: BTreeMap<String, Table>,
    pub failures: BTreeMap<&'static str, usize>,
}
#[derive(Default)]
pub struct Table {
    pub population: usize,
    pub counts: BTreeMap<&'static str, usize>,
}
impl Table {
    fn new(population: usize, labels: &[&'static str]) -> Self {
        Self {
            population,
            counts: labels.iter().map(|label| (*label, 0)).collect(),
        }
    }
    fn add(&mut self, label: &'static str) {
        *self.counts.entry(label).or_default() += 1;
    }
}
impl Summary {
    fn evaluate<T, E>(&mut self, operation: &'static str, result: Result<T, E>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(_) => {
                *self.failures.entry(operation).or_default() += 1;
                None
            }
        }
    }
    pub fn render(&self) -> String {
        use std::fmt::Write;
        let mut output = String::from("Aggregate readiness counts; shares suppressed below 5 rows in the denominator population.\n");
        if self.failures.is_empty() {
            output.push_str("Evaluation status: complete; no evaluation failed.\n");
        } else {
            output.push_str("Evaluation status: INCOMPLETE; evaluation failed. Affected populations or results may be incomplete.\n");
            for (operation, count) in &self.failures {
                writeln!(output, "{operation}: evaluation_failed={count}").unwrap();
            }
        }
        for (heading, table) in &self.tables {
            writeln!(output, "\n{heading}: population={}", table.population).unwrap();
            for (label, count) in &table.counts {
                writeln!(
                    output,
                    "  {label}: count={count}; share={}",
                    aggregate_share(*count, table.population)
                )
                .unwrap();
            }
        }
        output
    }
}

/// All I/O is repository reads; novel evidence evaluations stay in private pure helpers.
pub async fn measure(pool: &SqlitePool, clock: &Clock) -> Result<Summary, &'static str> {
    measure_as_of(pool, clock, clock.today()).await
}

pub async fn measure_as_of(
    pool: &SqlitePool,
    clock: &Clock,
    today: chrono::NaiveDate,
) -> Result<Summary, &'static str> {
    let mut summary = Summary::default();
    let mut held = Vec::new();
    let mut buy_more_years = BTreeMap::<i32, (usize, usize)>::new();
    let mut ever_held = BTreeSet::new();
    let instrument_rows = instruments::list(pool)
        .await
        .map_err(|_| "Measurement failed: R1 instrument inventory.")?;
    for instrument in instrument_rows {
        let Some(ledger) = summary.evaluate(
            "R1-R9 instrument ledger",
            transactions::ledger_for_instrument(pool, instrument.id).await,
        ) else {
            continue;
        };
        let past: Vec<_> = ledger
            .iter()
            .filter(|t| t.trade_date <= today)
            .cloned()
            .collect();
        // Use domain position derivation before each buy, including same-day id order.
        for (index, trade) in past
            .iter()
            .enumerate()
            .filter(|(_, t)| t.kind == TransactionKind::Buy)
        {
            ever_held.insert(instrument.id);
            let entry = buy_more_years.entry(trade.trade_date.year()).or_default();
            entry.0 += 1;
            if let Some(position) =
                summary.evaluate("R9 buy position", derive_position(&past[..index]))
            {
                if position.quantity > 0 {
                    entry.1 += 1;
                }
            }
        }
        // Match the app's current holdings universe (the complete stored ledger).
        let Some(position) = summary.evaluate("R1-R8 held position", derive_position(&ledger))
        else {
            continue;
        };
        if position.quantity > 0 {
            held.push((instrument, ledger, position));
        }
    }
    let total = held.len();
    let mut classification = Table::new(
        total,
        &[
            "company",
            "fund_or_etf",
            "certificate",
            "other_non_company",
            "unknown_or_no_source",
        ],
    );
    let mut markets = Table::new(total, history::MARKETS);
    let mut ex_dividend = Table::new(
        total,
        &[
            "yahoo_priced",
            "nasdaq_only",
            "other_source",
            "nasdaq_priced_with_yahoo_mapping",
            "no_latest_price",
        ],
    );
    let mut price_history = Table::new(total, history::HISTORY_LABELS);
    let mut companies = 0;
    let mut report_knowledge = [
        Table::new(0, reports::KNOWLEDGE_LABELS),
        Table::new(0, reports::KNOWLEDGE_LABELS),
    ];
    let mut report_basis = Table::new(
        0,
        &[
            "company_announced",
            "nasdaq_expected",
            "last_year_date",
            "typical_gap",
            "unavailable",
        ],
    );
    let mut fetch_age = Table::new(
        0,
        &[
            "0_to_1_days",
            "2_to_7_days",
            "over_7_days",
            "never_successful",
            "future_timestamp",
        ],
    );
    let mut business = Table::new(0, &["intact", "weakening", "loss_making_only", "unknown"]);
    let mut business_reasons = Table::default();
    let mut eps = Table::new(
        0,
        &[
            "consistent",
            "approximate",
            "unknown_inconsistent",
            "not_cross_checked",
            "pair_unavailable",
        ],
    );
    let mut timing: Vec<_> = WINDOWS
        .iter()
        .flat_map(|window| {
            [false, true].map(|relaxed| {
                (
                    *window,
                    relaxed,
                    Table::new(0, &["yes", "no", "unknown", "answered"]),
                    Table::default(),
                    Table::new(0, &["answered"]),
                )
            })
        })
        .collect();
    let mut changes: Vec<_> = WINDOWS
        .iter()
        .map(|window| {
            (
                *window,
                Table::new(
                    total,
                    &["available", "unavailable", "median_reference_ready"],
                ),
            )
        })
        .collect();

    for (instrument, ledger, position) in &held {
        let Some(mappings) = summary.evaluate(
            "R1-R8 price mappings",
            effective_prices::enabled_price_sources(pool, instrument.id).await,
        ) else {
            continue;
        };
        let Some(latest) = summary.evaluate(
            "R1-R8 latest price",
            effective_prices::effective_latest_on_or_before(pool, instrument.id, today).await,
        ) else {
            continue;
        };
        let valued = value_position(
            position,
            &instrument.currency,
            today,
            latest,
            None,
            None,
            None,
        );
        let snapshot = valued.latest_price.as_ref();
        let market = history::listing_market(&mappings, &instrument.currency);
        markets.add(market);
        let ledger_splits: Vec<_> = ledger
            .iter()
            .filter(|t| t.kind == TransactionKind::Split)
            .map(|t| t.trade_date)
            .collect();
        let split_result = db::fundamental_values::active_splits(pool, instrument.id).await;
        if let Some(provider_splits) = summary.evaluate("R6 split inputs", split_result) {
            let mut splits = ledger_splits.clone();
            splits.extend(provider_splits.iter().map(|s| s.date));
            // Commit a row's counters only after the entire history evaluation succeeds.
            let mut row_history = Table::new(1, history::HISTORY_LABELS);
            if summary
                .evaluate(
                    "R6 price history",
                    history::measure_history(
                        pool,
                        instrument,
                        today,
                        snapshot,
                        &mappings,
                        &splits,
                        &mut row_history,
                    )
                    .await,
                )
                .is_some()
            {
                for (label, count) in row_history.counts {
                    *price_history.counts.entry(label).or_default() += count;
                }
            }
        }
        ex_dividend.add(history::dividend_source(
            snapshot.map(|p| p.source.as_str()),
            &mappings,
        ));
        for (window, table) in &mut changes {
            let Some(start) = summary.evaluate(
                match window {
                    RecentWindow::OneWeek => "R8 7d change",
                    RecentWindow::TwoWeeks => "R8 14d change",
                    RecentWindow::ThirtyDays => "R8 30d change",
                },
                effective_prices::effective_latest_on_or_before(
                    pool,
                    instrument.id,
                    window.start_date(today),
                )
                .await,
            ) else {
                continue;
            };
            let change = recent_price_change(
                window.start_date(today),
                snapshot,
                start.as_ref(),
                &ledger_splits,
            );
            table.add(if matches!(change, RecentPriceChange::Available { .. }) {
                "available"
            } else {
                "unavailable"
            });
        }
        let Some(loaded) = summary.evaluate(
            "R1-R5 company inputs",
            load_company_figure_inputs(
                pool,
                instrument,
                today,
                clock,
                snapshot,
                None,
                CompanyFigureScope::Evidence(ledger),
            )
            .await,
        ) else {
            continue;
        };
        let company = matches!(
            loaded.source.classification,
            db::fundamentals_sources::Classification::Equity
                | db::fundamentals_sources::Classification::Figures
        );
        classification.add(if company {
            "company"
        } else {
            match loaded.source.classification {
                db::fundamentals_sources::Classification::Fund => "fund_or_etf",
                db::fundamentals_sources::Classification::Certificate => "certificate",
                db::fundamentals_sources::Classification::NotCompany => "other_non_company",
                _ => "unknown_or_no_source",
            }
        });
        if !company {
            continue;
        }
        companies += 1;
        report_basis.add(reports::basis(&loaded.next_report));
        fetch_age.add(
            match loaded.source.last_success.map(|d| (today - d).num_days()) {
                None => "never_successful",
                Some(0..=1) => "0_to_1_days",
                Some(2..=7) => "2_to_7_days",
                Some(8..) => "over_7_days",
                _ => "future_timestamp",
            },
        );
        {
            let inputs = reports::ReportInputs::from_loaded(today, &loaded);
            let region = usize::from(market != "us");
            report_knowledge[region].population += 1;
            report_knowledge[region].add(reports::knowledge(&inputs));
            for (window, relaxed, table, reasons, excluding_first_fetch) in &mut timing {
                let result = reports::in_period(&inputs, *window, *relaxed);
                table.add(result.state);
                if result.state != "unknown" {
                    table.add("answered");
                } else {
                    reasons.population += 1;
                    reasons.add(result.reason);
                }
                if result.reason != "first_fetch_without_baseline" {
                    excluding_first_fetch.population += 1;
                    if result.state != "unknown" {
                        excluding_first_fetch.add("answered");
                    }
                }
            }
        }
        let result = business::evaluate(today, &loaded);
        business.add(result.state);
        eps.add(result.eps);
        if result.state == "unknown" {
            business_reasons.population += 1;
            for reason in result.reasons {
                business_reasons.add(reason);
            }
        }
    }
    summary
        .tables
        .insert("R1 classification".into(), classification);
    summary.tables.insert("R1 listing_market".into(), markets);
    for (region, table) in ["us", "non_us"].into_iter().zip(report_knowledge) {
        summary
            .tables
            .insert(format!("R2 last_report {region}"), table);
    }
    for (window, relaxed, mut table, reasons, excluding_first_fetch) in timing {
        table.population = companies;
        let rule = if relaxed { "relaxed" } else { "strict" };
        summary
            .tables
            .insert(format!("R3 {} {rule}", window.as_str()), table);
        if window == RecentWindow::ThirtyDays {
            summary.tables.insert(
                format!("R3 30d {rule} excluding_first_fetch"),
                excluding_first_fetch,
            );
        }
        summary.tables.insert(
            format!("R3 {} {rule} unknown_reasons", window.as_str()),
            reasons,
        );
    }
    report_basis.population = companies;
    fetch_age.population = companies;
    business.population = companies;
    eps.population = companies;
    summary
        .tables
        .insert("R4 next_report_basis".into(), report_basis);
    summary
        .tables
        .insert("R4 successful_fetch_age".into(), fetch_age);
    summary.tables.insert("R5 business".into(), business);
    summary
        .tables
        .insert("R5 unknown_reasons".into(), business_reasons);
    summary.tables.insert("R5 eps_pair_consistency".into(), eps);
    summary.tables.insert("R6 history".into(), price_history);
    summary
        .tables
        .insert("R7 ex_dividend_source".into(), ex_dividend);
    for (window, mut table) in changes {
        if table.counts["available"] >= 6 {
            table
                .counts
                .insert("median_reference_ready", table.counts["available"]);
        }
        summary
            .tables
            .insert(format!("R8 {} change", window.as_str()), table);
    }
    let all_buys = buy_more_years.values().map(|(buys, _)| *buys).sum();
    let all_buy_mores = buy_more_years
        .values()
        .map(|(_, buy_mores)| *buy_mores)
        .sum();
    summary.tables.insert(
        "R9 buys all_years".into(),
        Table {
            population: all_buys,
            counts: BTreeMap::from([("buy_more", all_buy_mores)]),
        },
    );
    for (year, (buys, buy_mores)) in buy_more_years {
        summary.tables.insert(
            format!("R9 buys year={year}"),
            Table {
                population: buys,
                counts: BTreeMap::from([("buy_more", buy_mores)]),
            },
        );
    }
    let mut finality = Table::new(0, &["final_close", "non_final_close"]);
    let price_keys = sqlx::query_as::<_, (i64, i64)>("SELECT id, instrument_id FROM prices")
        .fetch_all(pool)
        .await
        .map_err(|_| "Measurement failed: R9 close inventory.")?;
    for (id, instrument_id) in price_keys {
        if !ever_held.contains(&instrument_id) {
            continue;
        }
        let result = async {
            let row = prices::find(pool, id)
                .await?
                .ok_or("Missing stored close")?;
            if row.date_value()? > today {
                return Ok::<_, Error>(None);
            }
            Ok(Some(history::final_close(&row)?))
        }
        .await;
        match summary.evaluate("R9 close finality", result) {
            Some(None) => continue,
            Some(Some(is_final)) => finality.add(if is_final {
                "final_close"
            } else {
                "non_final_close"
            }),
            None => finality.add("evaluation_failed"),
        }
        finality.population += 1;
    }
    summary
        .tables
        .insert("R9 stored_close_finality".into(), finality);
    Ok(summary)
}

/// The launch snapshot precedes the next refresh. Its latest terminal latest-mode run
/// is the witness for the inventory's local measurement date, including partial failures.
pub async fn ledger_measurement_date(
    pool: &SqlitePool,
    clock: &Clock,
    as_of: Option<chrono::NaiveDate>,
) -> Result<chrono::NaiveDate, &'static str> {
    if let Some(date) = as_of {
        return Ok(date);
    }
    let timestamps = sqlx::query_scalar::<_, String>(
        "SELECT finished_at FROM market_data_refresh_runs WHERE \"trigger\" IN ('LAUNCH', 'MANUAL') AND status IN ('SUCCEEDED', 'PARTIAL', 'FAILED') AND finished_at IS NOT NULL"
    ).fetch_all(pool).await.map_err(|_| "Measurement failed: snapshot refresh date lookup.")?;
    let latest = timestamps
        .iter()
        .map(|at| chrono::DateTime::parse_from_rfc3339(at))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Measurement failed: snapshot refresh date decoding.")?
        .into_iter()
        .max()
        .ok_or("Snapshot has no completed latest refresh; an explicit --as-of is required.")?;
    Ok(clock.local_date(latest.into()))
}

pub fn date_offset(clock: &Clock, measurement: chrono::NaiveDate) -> String {
    format!(
        "Clock today minus measurement date: {} days.\n",
        (clock.today() - measurement).num_days()
    )
}
