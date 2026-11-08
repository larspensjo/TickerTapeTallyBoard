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

pub(super) async fn seed(
    pool: &SqlitePool,
    today: NaiveDate,
    data: &DemoData,
    ids: &[i64],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    for (index, instrument) in data.instruments.iter().enumerate() {
        let symbol = instrument.symbol;
        let classification = if symbol == "GLBL" {
            Classification::Fund
        } else {
            Classification::Equity
        };
        let status = match symbol {
            "GLBL" => LinkStatus::NotApplicable,
            "VARV" => LinkStatus::NoMatch,
            _ => LinkStatus::Usable,
        };
        let mut values = Vec::new();
        let latest = month_end(today - Duration::days(if symbol == "SKOG" { 175 } else { 65 }))
            .expect("supported demo month end");
        let currency = if symbol == "NORD" {
            "EUR"
        } else {
            instrument.currency
        };
        if status == LinkStatus::Usable {
            for n in 0..5_u32 {
                let period_end =
                    month_end(latest.checked_sub_months(Months::new((4 - n) * 3)).unwrap())
                        .expect("supported demo quarter end");
                if symbol == "SKOG" && n == 2 {
                    continue;
                }
                let income = if symbol == "ALBA" {
                    dec!(-100)
                } else {
                    dec!(100)
                };
                for (metric, value) in [
                    (Metric::Revenue, dec!(1000) + Decimal::from(n) * dec!(50)),
                    (Metric::DilutedNetIncome, income),
                    (Metric::DilutedShares, dec!(100)),
                    (Metric::DilutedEps, income / dec!(100)),
                ] {
                    if symbol == "SKOG" && n == 3 && metric == Metric::DilutedEps {
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
                    / if symbol == "HARV" {
                        dec!(5)
                    } else {
                        Decimal::ONE
                    },
                currency: Some(instrument.currency.into()),
            });
        }
        let mut write = FetchWrite {
            instrument_id: ids[index],
            expected: None,
            discovery: Some(LinkDefinition {
                source: Some(FundamentalsSource::Yahoo),
                symbol: (symbol != "VARV").then(|| instrument.provider_symbol.into()),
                security_type: Some(if symbol == "GLBL" { "ETF" } else { "EQUITY" }.into()),
                classification,
                origin: LinkOrigin::Discovered,
                status,
                candidates: "[]".into(),
            }),
            values,
            splits: vec![],
            at: format!("{today}T12:00:00Z"),
            seen_on: today - Duration::days(10),
            previous_success_on: None,
            status: AttemptStatus::Succeeded,
            reason: None,
        };
        let newest: Vec<_> = write
            .values
            .iter()
            .filter(|v| v.period_end == latest)
            .cloned()
            .collect();
        if symbol == "NOVA" {
            write.values.retain(|v| v.period_end != latest);
            write.seen_on = today - Duration::days(14);
        }
        let mut tx = pool.begin().await?;
        let GenerationWrite::Applied(link) =
            fundamental_values::write_fetch(&mut tx, &write).await?
        else {
            return Err("demo source superseded".into());
        };
        tx.commit().await?;
        if symbol == "NOVA" {
            write.discovery = None;
            write.expected = Some(link.token);
            write.values = newest;
            write.previous_success_on = Some(write.seen_on);
            write.seen_on = today - Duration::days(10);
            let mut tx = pool.begin().await?;
            fundamental_values::write_fetch(&mut tx, &write).await?;
            tx.commit().await?;
        }
    }
    Ok(())
}
