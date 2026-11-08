use chrono::{DateTime, NaiveDate};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use ticker_tape_tally_board_backend::{
    domain::{
        company_figures::{
            compute, Certainty, FxRate, Inputs, Price, ReportedValue, ShareBasisState, SourceState,
        },
        fundamentals::{Classification, LinkStatus, Metric},
    },
    providers::yahoo_fundamentals::YahooFundamentalsClient,
};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/fundamentals/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

#[test]
fn recorded_company_figures_match_calibration_and_tsm_receipt_basis() {
    let today: NaiveDate = "2026-11-04".parse().unwrap();
    let source = SourceState {
        classification: Classification::Equity,
        status: LinkStatus::Usable,
        awaiting_figures: false,
        unreachable: false,
        last_success: None,
    };
    let rates = vec![FxRate {
        currency: "EUR".into(),
        date: today,
        sek_per_unit: dec!(10.9718),
    }];
    for (symbol, pe, ps, margin) in [
        ("msft", dec!(28.9), dec!(11.90), dec!(0.412)),
        ("asml_as", dec!(38.0), dec!(12.92), dec!(0.341)),
        ("evo_st", dec!(14.6), dec!(6.96), dec!(0.496)),
        ("volv_b_st", dec!(16.3), dec!(1.33), dec!(0.081)),
        ("volv_a_st", dec!(16.3), dec!(1.33), dec!(0.081)),
        ("tsm", dec!(26.5), dec!(13.65), dec!(0.511)),
    ] {
        let values = YahooFundamentalsClient::parse_response(&fixture(&format!(
            "yahoo_timeseries_{symbol}.json"
        )))
        .unwrap()
        .into_iter()
        .map(|input| ReportedValue {
            input,
            fetched_on: today,
        })
        .collect::<Vec<_>>();
        let raw: serde_json::Value =
            serde_json::from_str(&fixture(&format!("yahoo_chart_{symbol}.json"))).unwrap();
        let chart = &raw["chart"]["result"][0];
        let prices = chart["timestamp"]
            .as_array()
            .unwrap()
            .iter()
            .zip(chart["indicators"]["quote"][0]["close"].as_array().unwrap())
            .map(|(t, p)| Price {
                date: DateTime::from_timestamp(t.as_i64().unwrap(), 0)
                    .unwrap()
                    .date_naive(),
                close: p.to_string().parse().unwrap(),
                currency: chart["meta"]["currency"].as_str().unwrap().into(),
                stale: false,
            })
            .collect::<Vec<_>>();
        let price = prices.last().unwrap();
        let evidence_date = values
            .iter()
            .filter(|v| v.input.metric == Metric::MarketCap)
            .map(|v| v.input.period_end)
            .max()
            .unwrap();
        let evidence_price = prices
            .iter()
            .rev()
            .find(|p| p.date <= evidence_date)
            .unwrap();
        let f = compute(&Inputs {
            today,
            values: &values,
            splits: &[],
            price: Some(price),
            evidence_price: Some(evidence_price),
            fx: &rates,
            source: &source,
            next_report: None,
        });
        assert_eq!(
            f.share_basis.state,
            ShareBasisState::Established,
            "{symbol}: {:?}",
            f.share_basis
        );
        assert!(
            (f.profit_margin.value.unwrap() - margin).abs() < dec!(0.001),
            "{symbol}: {:?}",
            f.profit_margin
        );
        let latest = &f.quarters[f.quarters.len() - 1].sales;
        let ago = &f.quarters[f.quarters.len() - 5].sales;
        if let Some(ago) = ago.value {
            assert_eq!(
                f.sales_growth.value,
                Some(latest.value.unwrap() / ago - Decimal::ONE)
            );
        } else {
            assert_eq!(f.sales_growth.reasons[0].code, "year_ago_quarter_missing");
        }
        if symbol == "tsm" {
            assert!(f.pe.value.is_none());
            assert!(f.ps.value.is_none());
            assert_eq!(f.pe.reasons[0].code, "fx_unavailable");
            assert_eq!(f.pe.reasons[0].detail.as_deref(), Some("TWD"));
        } else {
            assert_eq!(f.pe.certainty, Certainty::Complete, "{symbol}: {:?}", f.pe);
            assert_eq!(f.ps.value.unwrap().round_dp(2), ps, "{symbol}");
            assert_eq!(f.pe.value.unwrap().round_dp(1), pe, "{symbol}");
        }
        if symbol == "evo_st" {
            assert!(f.quarters.iter().any(|q| q.missing));
            assert_eq!(f.fx_conversion.unwrap().rate, dec!(10.9718));
        }
    }
}
