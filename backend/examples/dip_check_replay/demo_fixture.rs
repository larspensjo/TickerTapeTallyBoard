use super::*;
use ticker_tape_tally_board_backend::ledger_analysis::replay_inputs::resolve_closes;

/// Extend a seeded demo row into a dedicated, deterministic replay fixture.
/// Does not change the application's demo seed or write any file-backed ledger.
pub fn demo_fixture(
    seed: Vec<ReplayInstrument>,
    today: NaiveDate,
    truncated: bool,
) -> Result<Vec<ReplayInstrument>, &'static str> {
    let template = seed
        .into_iter()
        .find(|i| i.stored.iter().any(|c| c.provider.as_str() == "YAHOO"))
        .ok_or("Demo replay template unavailable.")?;
    let close_template = template
        .stored
        .iter()
        .find(|c| c.provider.as_str() == "YAHOO")
        .ok_or("Demo replay close template unavailable.")?;
    let start = today - Duration::days(if truncated { 140 } else { 1000 });
    let mut instruments = Vec::new();
    for index in 0..12 {
        let mut instrument = template.clone();
        instrument.key = index + 1;
        instrument.ledger_splits.clear();
        instrument.provider_splits.clear();
        instrument.observed_eps.clear();
        instrument.stored.clear();
        instrument.ledger.clear();
        let mut date = start;
        let mut n = 0;
        while date <= today {
            if !matches!(date.weekday(), Weekday::Sat | Weekday::Sun) {
                let mut close = close_template.clone();
                close.date = date;
                close.currency = instrument.currency.clone();
                let phase = (n + index * 5) % 47;
                let wave = if phase < 24 { phase } else { 47 - phase };
                close.close =
                    dec!(600) - Decimal::from(n) * dec!(0.55) + Decimal::from(wave) * dec!(0.8);
                close.fetched_at = (today + Duration::days(1))
                    .and_hms_opt(0, 0, 0)
                    .ok_or("Demo date unavailable.")?
                    .and_utc();
                instrument.stored.push(close);
                if n % 40 == 0 {
                    instrument.ledger.push(LedgerTransaction {
                        id: n + 1,
                        trade_date: date,
                        kind: TransactionKind::Buy,
                        quantity: 1,
                        price: Some(dec!(1)),
                        dividend_per_share: None,
                        fx_rate_to_base: Some(dec!(1)),
                        brokerage_base: Decimal::ZERO,
                    });
                }
                n += 1;
            }
            date += Duration::days(1);
        }
        instrument.sessions = resolve_closes(&instrument.stored, false);
        instrument.finals = resolve_closes(&instrument.stored, true);
        instruments.push(instrument);
    }
    Ok(instruments)
}
