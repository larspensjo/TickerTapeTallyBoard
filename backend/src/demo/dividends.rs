use super::DemoData;
use crate::{
    db::{dividends, RepoError},
    domain::opportunity_evidence::dividend_check::DividendEvent,
    providers::DividendCapture,
};
use chrono::{Duration, NaiveDate};
use rust_decimal_macros::dec;
use sqlx::SqlitePool;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DividendCase {
    Uncovered,
    Invalid,
    ExDividend,
    Covered,
}

fn capture(case: DividendCase, currency: &str, today: NaiveDate) -> Option<DividendCapture> {
    match case {
        // An uncovered interval remains distinct from a failed capture.
        DividendCase::Uncovered => None,
        DividendCase::Invalid => Some(DividendCapture::Invalid {
            reason: "demo invalid event amount".into(),
        }),
        DividendCase::ExDividend => Some(DividendCapture::Valid(vec![DividendEvent {
            ex_date: today - Duration::days(3),
            amount: dec!(0.5),
            currency: currency.into(),
        }])),
        DividendCase::Covered => Some(DividendCapture::Valid(vec![])),
    }
}

pub(super) async fn seed(
    pool: &SqlitePool,
    today: NaiveDate,
    data: &DemoData,
    ids: &[i64],
) -> Result<(), RepoError> {
    let mut tx = pool.begin().await?;
    for (index, instrument) in data.instruments.iter().enumerate() {
        if let Some(capture) = capture(instrument.scenario.dividends, instrument.currency, today) {
            dividends::capture(
                &mut tx,
                ids[index],
                instrument.provider,
                (today - Duration::days(40))..=today,
                Some(today),
                &capture,
            )
            .await?;
        }
    }
    tx.commit().await?;
    Ok(())
}
