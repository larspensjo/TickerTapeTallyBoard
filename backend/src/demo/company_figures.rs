use super::DemoData;
use crate::domain::calendar::month_end;
use crate::{
    db::{
        fundamental_values::{self, FetchWrite},
        fundamentals_sources::{AttemptStatus, GenerationWrite, LinkDefinition, LinkOrigin},
    },
    domain::fundamentals::{
        Classification, FundamentalValue, FundamentalsSource, LinkStatus, Metric, PeriodType,
    },
};
use chrono::{Duration, Months, NaiveDate};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use sqlx::SqlitePool;

/// Company-side facts independent of instrument identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FigureCase {
    Recovery,
    ApproximateEps,
    ForeignReports,
    SplitStale,
    StableLoss,
    Fund,
    NoSource,
    RecentListing,
    WeakeningLoss,
}

pub(super) async fn seed(
    pool: &SqlitePool,
    today: NaiveDate,
    data: &DemoData,
    ids: &[i64],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    for (index, instrument) in data.instruments.iter().enumerate() {
        let (latest, captures) = figure_captures(today, data, index, ids[index]);
        let mut token = None;
        for mut capture in captures {
            capture.expected = token;
            let mut tx = pool.begin().await?;
            let GenerationWrite::Applied(link) =
                fundamental_values::write_fetch(&mut tx, &capture).await?
            else {
                return Err(
                    format!("demo company figures superseded for {}", instrument.symbol).into(),
                );
            };
            tx.commit().await?;
            token = Some(link.token);
        }
        seed_report_dates(pool, ids[index], instrument, today, latest).await?;
    }
    Ok(())
}

/// Captures, including their absence baselines, are determined entirely by today.
fn figure_captures(
    today: NaiveDate,
    data: &DemoData,
    index: usize,
    id: i64,
) -> (NaiveDate, Vec<FetchWrite>) {
    let instrument = &data.instruments[index];
    let case = instrument.scenario.figures;
    let classification = if case == FigureCase::Fund {
        Classification::Fund
    } else {
        Classification::Equity
    };
    let status = match case {
        FigureCase::Fund => LinkStatus::NotApplicable,
        FigureCase::NoSource => LinkStatus::NoMatch,
        _ => LinkStatus::Usable,
    };
    let mut values = Vec::new();
    let latest = month_end(
        today
            - Duration::days(match case {
                FigureCase::SplitStale => 175,
                FigureCase::StableLoss => 130,
                _ => 65,
            }),
    )
    .expect("supported demo month end");
    let currency = if case == FigureCase::ForeignReports {
        "EUR"
    } else {
        instrument.currency
    };
    if status == LinkStatus::Usable {
        for n in 0..5_u32 {
            let period_end =
                month_end(latest.checked_sub_months(Months::new((4 - n) * 3)).unwrap())
                    .expect("supported demo quarter end");
            if case == FigureCase::SplitStale && n == 2 {
                continue;
            }
            let income = if matches!(case, FigureCase::StableLoss | FigureCase::WeakeningLoss) {
                if case == FigureCase::WeakeningLoss {
                    dec!(-100) - Decimal::from(n) * dec!(20)
                } else {
                    dec!(-100)
                }
            } else {
                dec!(100)
            };
            for (metric, value) in [
                (
                    Metric::Revenue,
                    dec!(1000)
                        + Decimal::from(n)
                            * if case == FigureCase::WeakeningLoss {
                                dec!(-50)
                            } else {
                                dec!(50)
                            },
                ),
                (Metric::DilutedNetIncome, income),
                (Metric::DilutedShares, dec!(100)),
                (
                    Metric::DilutedEps,
                    income / dec!(100)
                        * match case {
                            FigureCase::ApproximateEps => dec!(1.12), // usable, approximate EPS comparison
                            FigureCase::RecentListing => dec!(1.5), // severe disagreement, unknown business
                            _ => Decimal::ONE,
                        },
                ),
            ] {
                if case == FigureCase::SplitStale && n == 3 && metric == Metric::DilutedEps {
                    continue;
                }
                // Missing year-ago shares prevents an EPS cross-check without
                // removing the reported EPS or changing trailing-ratio certainty.
                if case == FigureCase::ForeignReports && n == 0 && metric == Metric::DilutedShares {
                    continue;
                }
                values.push(FundamentalValue {
                    period_type: PeriodType::Quarter,
                    period_end,
                    metric,
                    value,
                    currency: Some(currency.into()),
                });
            }
        }
        let price = data
            .prices
            .iter()
            .find(|p| p.instrument_index == index && p.date == today)
            .unwrap();
        values.push(FundamentalValue {
            period_type: PeriodType::Evidence,
            period_end: today,
            metric: Metric::MarketCap,
            value: price.close * dec!(100)
                / if case == FigureCase::ApproximateEps {
                    dec!(5)
                } else {
                    Decimal::ONE
                },
            currency: Some(instrument.currency.into()),
        });
    }
    let write = FetchWrite {
        instrument_id: id,
        expected: None,
        verified: None,
        discovery: Some(LinkDefinition {
            source: Some(FundamentalsSource::Yahoo),
            symbol: (case != FigureCase::NoSource).then(|| instrument.provider_symbol.into()),
            security_type: Some(
                if case == FigureCase::Fund {
                    "ETF"
                } else {
                    "EQUITY"
                }
                .into(),
            ),
            classification,
            origin: LinkOrigin::Discovered,
            status,
            candidates: "[]".into(),
        }),
        values,
        splits: if case == FigureCase::SplitStale {
            vec![crate::domain::fundamentals::SplitEvent {
                date: today - Duration::days(super::price_paths::SPLIT_AGE_DAYS),
                numerator: dec!(2),
                denominator: dec!(1),
            }]
        } else {
            vec![]
        },
        at: format!("{today}T12:00:00Z"),
        seen_on: today
            - Duration::days(if case == FigureCase::StableLoss {
                50
            } else {
                10
            }),
        previous_success_on: None,
        status: AttemptStatus::Succeeded,
        reason: None,
    };

    let mut captures = Vec::new();
    let mut current = write;
    if case == FigureCase::StableLoss {
        // This quarter's year-ago report was observed within four days. Its
        // midpoint projects to four days ahead, with the app's estimate label.
        let period = month_end(latest.checked_sub_months(Months::new(9)).unwrap()).unwrap();
        let historical: Vec<_> = current
            .values
            .iter()
            .filter(|v| v.period_end == period)
            .cloned()
            .collect();
        current.values.retain(|v| v.period_end != period);
        captures.push(FetchWrite {
            instrument_id: id,
            expected: None,
            verified: None,
            discovery: current.discovery.take(),
            values: vec![],
            splits: vec![],
            at: format!("{}T12:00:00Z", today - Duration::days(362)),
            seen_on: today - Duration::days(362),
            previous_success_on: None,
            status: AttemptStatus::Succeeded,
            reason: None,
        });
        captures.push(FetchWrite {
            instrument_id: id,
            expected: None,
            verified: None,
            discovery: None,
            values: historical,
            splits: vec![],
            at: format!("{}T12:00:00Z", today - Duration::days(358)),
            seen_on: today - Duration::days(358),
            previous_success_on: Some(today - Duration::days(362)),
            status: AttemptStatus::Succeeded,
            reason: None,
        });
    }
    if matches!(case, FigureCase::Recovery | FigureCase::RecentListing) {
        let newest: Vec<_> = current
            .values
            .iter()
            .filter(|v| v.period_end == latest)
            .cloned()
            .collect();
        current.values.retain(|v| v.period_end != latest);
        current.seen_on = today
            - Duration::days(if case == FigureCase::RecentListing {
                14
            } else {
                25
            });
        let seen = current.seen_on;
        captures.push(current);
        current = FetchWrite {
            instrument_id: id,
            expected: None,
            verified: None,
            discovery: None,
            values: newest,
            splits: vec![],
            at: format!("{today}T12:00:00Z"),
            seen_on: today
                - Duration::days(if case == FigureCase::RecentListing {
                    10
                } else {
                    21
                }),
            previous_success_on: Some(seen),
            status: AttemptStatus::Succeeded,
            reason: None,
        };
    }
    captures.push(current);
    (latest, captures)
}

/// Pure schedules: the recorded announcement day is separate from report day.
fn report_schedules(
    instrument: &super::DemoInstrument,
    today: NaiveDate,
    latest: NaiveDate,
) -> Vec<(NaiveDate, crate::domain::report_dates::AnnouncementDate)> {
    use crate::domain::report_dates::{AnnouncementDate, AnnouncementKind, ReportSession};
    let case = instrument.scenario.figures;
    let (offset, kind) = match case {
        FigureCase::Recovery => (3, AnnouncementKind::CompanyAnnounced),
        FigureCase::ApproximateEps => (4, AnnouncementKind::Expected),
        FigureCase::ForeignReports => (-3, AnnouncementKind::CompanyAnnounced),
        _ => return vec![],
    };
    let mut schedules = vec![(
        today,
        AnnouncementDate {
            date: today + Duration::days(offset),
            kind,
            fiscal_quarter: month_end(latest.checked_add_months(Months::new(3)).unwrap()),
            session: (kind == AnnouncementKind::CompanyAnnounced)
                .then_some(ReportSession::AfterMarketClose),
        },
    )];
    if case == FigureCase::Recovery {
        // Figures first appeared twenty-one days ago, resolving this earlier
        // company-announced schedule. It is outside 7d but inside 30d evidence.
        schedules.push((
            today - Duration::days(30),
            AnnouncementDate {
                date: today - Duration::days(22),
                kind: AnnouncementKind::CompanyAnnounced,
                fiscal_quarter: Some(latest),
                session: Some(ReportSession::AfterMarketClose),
            },
        ));
    }
    schedules
}

/// Store offline schedules and resolve against the same observations as live data.
async fn seed_report_dates(
    pool: &SqlitePool,
    id: i64,
    instrument: &super::DemoInstrument,
    today: NaiveDate,
    latest: NaiveDate,
) -> Result<(), crate::db::RepoError> {
    use crate::{db::report_announcements as repo, domain::report_dates::*};
    let schedules = report_schedules(instrument, today, latest);
    if schedules.is_empty() {
        return Ok(());
    }
    let ticker = repo::UsTicker {
        symbol: instrument.symbol.into(),
        link_generation: None,
    };
    let mut tx = pool.begin().await?;
    for (announced_on, report) in schedules {
        repo::apply(
            &mut tx,
            id,
            &ticker,
            announced_on,
            &[AnnouncementChange::Insert(report)],
        )
        .await?;
    }
    let entries = repo::for_ticker(&mut *tx, id, &ticker).await?;
    let observations = repo::observations(&mut *tx, id).await?;
    repo::apply(
        &mut tx,
        id,
        &ticker,
        today,
        &resolve_announcements(&entries, &observations),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}
