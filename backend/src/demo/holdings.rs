use super::{
    company_figures::FigureCase, dividends::DividendCase, fx_rate_for_currency,
    price_paths::PricePath, DemoInstrument, DemoTransaction,
};

use crate::{domain::TransactionKind, providers::MarketDataProvider};
use chrono::{Duration, NaiveDate};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// One case definition per instrument keeps offline evidence inputs together.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DemoScenario {
    pub prices: PricePath,
    pub figures: FigureCase,
    pub dividends: DividendCase,
}

pub(super) fn instruments() -> Vec<DemoInstrument> {
    vec![
        DemoInstrument {
            symbol: "NOVA",
            scenario: DemoScenario {
                prices: PricePath {
                    scale: dec!(1),
                    end: dec!(72),
                    recovery: true,
                    ..PricePath::default()
                },
                figures: FigureCase::Recovery,
                dividends: DividendCase::ExDividend,
            },
            exchange: "NASDAQ",
            name: "Nova Systems",
            kind: "STOCK",
            currency: "USD",
            isin: None,
            provider: MarketDataProvider::Yahoo,
            asset_class: None,
            provider_symbol: "NOVA",
            conviction: "HIGH",
        },
        DemoInstrument {
            symbol: "HARV",
            scenario: DemoScenario {
                prices: PricePath {
                    scale: dec!(0.43),
                    end: dec!(90),
                    ..PricePath::default()
                },
                figures: FigureCase::ApproximateEps,
                dividends: DividendCase::Covered,
            },
            exchange: "NYSE",
            name: "Harbor Utilities",
            kind: "STOCK",
            currency: "USD",
            isin: None,
            provider: MarketDataProvider::Yahoo,
            asset_class: None,
            provider_symbol: "HARV",
            conviction: "LOW",
        },
        DemoInstrument {
            symbol: "NORD",
            scenario: DemoScenario {
                prices: PricePath {
                    scale: dec!(1.65),
                    end: dec!(78),
                    ..PricePath::default()
                },
                figures: FigureCase::ForeignReports,
                dividends: DividendCase::Covered,
            },
            exchange: "STO",
            name: "Nordic Automation",
            kind: "STOCK",
            currency: "SEK",
            isin: None,
            provider: MarketDataProvider::Yahoo,
            asset_class: None,
            // Synthetic suffix-free identity exercises the unresolved US date.
            provider_symbol: "NORD",
            conviction: "MEDIUM",
        },
        DemoInstrument {
            symbol: "SKOG",
            scenario: DemoScenario {
                prices: PricePath {
                    scale: dec!(0.91),
                    end: dec!(82),
                    split_stale: true,
                    ..PricePath::default()
                },
                figures: FigureCase::SplitStale,
                dividends: DividendCase::Invalid,
            },
            exchange: "STO",
            name: "Skog & Marin",
            kind: "STOCK",
            currency: "SEK",
            isin: None,
            provider: MarketDataProvider::Yahoo,
            asset_class: None,
            provider_symbol: "SKOG.ST",
            conviction: "OTHER",
        },
        DemoInstrument {
            symbol: "ALBA",
            scenario: DemoScenario {
                prices: PricePath {
                    scale: dec!(0.65),
                    end: dec!(65),
                    ..PricePath::default()
                },
                figures: FigureCase::StableLoss,
                dividends: DividendCase::Uncovered,
            },
            exchange: "XETRA",
            name: "Alba Renewables",
            kind: "STOCK",
            currency: "EUR",
            isin: None,
            provider: MarketDataProvider::Yahoo,
            asset_class: None,
            provider_symbol: "ALBA.DE",
            conviction: "OTHER",
        },
        DemoInstrument {
            symbol: "GLBL",
            scenario: DemoScenario {
                prices: PricePath {
                    scale: dec!(0.54),
                    end: dec!(99.7),
                    rising_week: true,
                    wave_percent: dec!(3),
                    ..PricePath::default()
                },
                figures: FigureCase::Fund,
                dividends: DividendCase::Covered,
            },
            exchange: "NYSEARCA",
            name: "Global Core ETF",
            kind: "ETF",
            currency: "USD",
            isin: None,
            provider: MarketDataProvider::Yahoo,
            asset_class: None,
            provider_symbol: "GLBL",
            conviction: "HIGH",
        },
        DemoInstrument {
            symbol: "VARV",
            scenario: DemoScenario {
                prices: PricePath {
                    scale: dec!(2.5),
                    end: dec!(85),
                    gap: Some((16, 27)),
                    ..PricePath::default()
                },
                figures: FigureCase::NoSource,
                dividends: DividendCase::Uncovered,
            },
            exchange: "AVANZA",
            name: "Varvsgatan Industri",
            kind: "STOCK",
            currency: "SEK",
            isin: Some("SE0009999901"),
            provider: MarketDataProvider::NasdaqNordic,
            asset_class: Some("SHARES"),
            provider_symbol: "TX9001",
            conviction: "MEDIUM",
        },
        DemoInstrument {
            symbol: "SPRO",
            scenario: DemoScenario {
                prices: PricePath {
                    scale: dec!(1),
                    end: dec!(80),
                    history_days: Some(80),
                    wave_percent: dec!(1),
                    ..PricePath::default()
                },
                figures: FigureCase::RecentListing,
                dividends: DividendCase::Covered,
            },
            exchange: "OSL",
            name: "Sprout Diagnostics",
            kind: "STOCK",
            currency: "NOK",
            isin: None,
            provider: MarketDataProvider::Yahoo,
            asset_class: None,
            provider_symbol: "SPRO.OL",
            conviction: "HIGH",
        },
        DemoInstrument {
            symbol: "BRIM",
            scenario: DemoScenario {
                prices: PricePath {
                    scale: dec!(1),
                    end: dec!(68),
                    ..PricePath::default()
                },
                figures: FigureCase::WeakeningLoss,
                dividends: DividendCase::Covered,
            },
            exchange: "STO",
            name: "Brim Materials",
            kind: "STOCK",
            currency: "SEK",
            isin: None,
            provider: MarketDataProvider::Yahoo,
            asset_class: None,
            provider_symbol: "BRIM.ST",
            conviction: "OTHER",
        },
    ]
}

pub(super) fn transactions(
    start_date: NaiveDate,
    end_date: NaiveDate,
    instruments: &[DemoInstrument],
) -> Vec<DemoTransaction> {
    let specs: [Vec<TransactionSpec>; 9] = [
        vec![
            buy(20, 24),
            dividend(118, dec!(0.42)),
            buy(168, 12),
            dividend(214, dec!(0.45)),
            dividend(304, dec!(0.47)),
            sell(356, 8),
            dividend(486, dec!(0.50)),
        ],
        vec![
            buy(45, 32),
            dividend(126, dec!(0.31)),
            buy(231, 18),
            dividend(273, dec!(0.33)),
            dividend(365, dec!(0.35)),
            dividend(512, dec!(0.37)),
        ],
        vec![
            buy(16, 18),
            dividend(151, dec!(2.10)),
            buy(238, 14),
            dividend(322, dec!(2.25)),
            buy(402, 10),
            dividend(525, dec!(2.40)),
        ],
        vec![
            buy(64, 40),
            dividend(184, dec!(1.15)),
            buy(276, 18),
            sell(431, 16),
            dividend(519, dec!(1.22)),
        ],
        vec![
            buy(34, 22),
            dividend(142, dec!(0.72)),
            buy(312, 12),
            dividend(394, dec!(0.76)),
            dividend(536, dec!(0.80)),
        ],
        vec![
            buy(82, 36),
            dividend(246, dec!(0.18)),
            buy(342, 20),
            dividend(526, dec!(0.20)),
        ],
        vec![
            buy(28, 14),
            dividend(163, dec!(3.10)),
            buy(299, 9),
            dividend(418, dec!(3.25)),
            dividend(533, dec!(3.40)),
        ],
        // Recently listed: no transactions predate its short price history.
        vec![buy(500, 10), dividend(530, dec!(0.10))],
        vec![
            buy(60, 20),
            dividend(240, dec!(0.20)),
            dividend(480, dec!(0.20)),
        ],
    ];

    specs
        .into_iter()
        .enumerate()
        .flat_map(|(instrument_index, specs)| {
            let instrument = &instruments[instrument_index];
            let mut specs = specs;
            if instrument.scenario.prices.split_stale {
                let day_offset =
                    (end_date - start_date).num_days() - super::price_paths::SPLIT_AGE_DAYS;
                // Match the provider's 2:1 split in the ledger and raw price history.
                let quantity = specs
                    .iter()
                    .filter(|s| s.day_offset < day_offset)
                    .map(|s| s.quantity)
                    .sum();
                specs.push(TransactionSpec {
                    kind: TransactionKind::Split,
                    day_offset,
                    quantity,
                    dividend_per_share: None,
                });
                specs.sort_by_key(|s| s.day_offset);
            }
            specs.into_iter().map(move |spec| {
                let trade_date = start_date + Duration::days(spec.day_offset);
                let is_trade = matches!(spec.kind, TransactionKind::Buy | TransactionKind::Sell);
                let has_currency = spec.kind != TransactionKind::Split;
                let fx_rate_to_base =
                    has_currency.then(|| fx_rate_for_currency(instrument.currency, trade_date));

                DemoTransaction {
                    instrument_index,
                    kind: spec.kind,
                    trade_date,
                    quantity: spec.quantity,
                    price: is_trade.then(|| {
                        super::price_paths::close(
                            &instrument.scenario.prices,
                            (end_date - trade_date).num_days(),
                        )
                    }),
                    dividend_per_share: spec.dividend_per_share,
                    currency: has_currency.then_some(instrument.currency),
                    fx_rate_to_base,
                    brokerage: is_trade.then_some(dec!(9.00)),
                    note: Some("Demo seed"),
                }
            })
        })
        .collect()
}

#[derive(Clone, Debug)]
struct TransactionSpec {
    kind: TransactionKind,
    day_offset: i64,
    quantity: i64,
    dividend_per_share: Option<Decimal>,
}

fn buy(day_offset: i64, quantity: i64) -> TransactionSpec {
    TransactionSpec {
        kind: TransactionKind::Buy,
        day_offset,
        quantity,
        dividend_per_share: None,
    }
}

fn sell(day_offset: i64, quantity: i64) -> TransactionSpec {
    TransactionSpec {
        kind: TransactionKind::Sell,
        day_offset,
        quantity: -quantity,
        dividend_per_share: None,
    }
}

fn dividend(day_offset: i64, dividend_per_share: Decimal) -> TransactionSpec {
    TransactionSpec {
        kind: TransactionKind::Dividend,
        day_offset,
        quantity: 0,
        dividend_per_share: Some(dividend_per_share),
    }
}
