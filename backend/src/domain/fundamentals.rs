//! Source-neutral stored inputs. Computations never depend on a provider client.
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

macro_rules! codes {
    ($(#[$attr:meta])* $name:ident { $($(#[$variant_attr:meta])* $variant:ident => $code:literal),+ $(,)? }) => {
        $(#[$attr])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
        pub enum $name { $($(#[$variant_attr])* #[serde(rename = $code)] $variant),+ }
        impl $name {
            pub fn as_str(self) -> &'static str { match self { $(Self::$variant => $code),+ } }
            pub fn from_db_str(code: &str) -> Option<Self> {
                match code { $($code => Some(Self::$variant)),+, _ => None }
            }
        }
    };
}

codes!(FundamentalsSource { Yahoo => "YAHOO" });
codes!(Metric {
    Revenue => "TOTAL_REVENUE", DilutedEps => "DILUTED_EPS",
    NetIncome => "NET_INCOME", CommonNetIncome => "COMMON_NET_INCOME",
    DilutedNetIncome => "DILUTED_NET_INCOME", DilutedShares => "DILUTED_SHARES",
    OrdinaryShares => "ORDINARY_SHARES", MarketCap => "MARKET_CAP"
});
codes!(PeriodType { Quarter => "QUARTER", Annual => "ANNUAL", Trailing => "TRAILING", Evidence => "EVIDENCE" });
codes!(LinkOrigin { Discovered => "DISCOVERED", Manual => "MANUAL" });
codes!(LinkStatus {
    Pending => "PENDING", Usable => "USABLE", NotApplicable => "NOT_APPLICABLE",
    Ambiguous => "AMBIGUOUS", NoMatch => "NO_MATCH", Unavailable => "UNAVAILABLE"
});
codes!(Classification {
    Equity => "EQUITY", Fund => "FUND", Certificate => "CERTIFICATE",
    NotCompany => "NOT_COMPANY", Unknown => "UNKNOWN", Figures => "FIGURES"
});
codes!(AttemptStatus { Succeeded => "SUCCEEDED", Empty => "EMPTY", Failed => "FAILED" });
codes!(#[derive(Default)] FundamentalsStatus {
    #[default] NotRun => "not_run", Succeeded => "succeeded", Partial => "partial", Failed => "failed"
});

#[derive(Clone, Debug, PartialEq)]
pub struct FundamentalValue {
    pub period_type: PeriodType,
    pub period_end: NaiveDate,
    pub metric: Metric,
    pub value: Decimal,
    pub currency: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SplitEvent {
    pub date: NaiveDate,
    pub numerator: Decimal,
    pub denominator: Decimal,
}
