use super::UnavailableReason;
use rust_decimal::Decimal;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValuationReason {
    MissingPrice,
    MissingFx,
    MissingPreviousClose,
    PreviousCloseSourceMismatch,
    MissingPreviousFx,
    StalePrice { trading_days: i64 },
    StaleFx { trading_days: i64 },
    ZeroCostBasis,
    ZeroPreviousMarketValue,
    BaseCostBasisUnavailable { reasons: Vec<UnavailableReason> },
    // performance-specific variants:
    MissingStartPrice,
    MissingEndPrice,
    MissingStartFx,
    MissingEndFx,
    MissingTransactionPrice { transaction_id: i64 },
    MissingTransactionFx { transaction_id: i64 },
    ZeroOrInvalidPerformanceDenominator,
    PerformanceDidNotConverge,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Availability<T> {
    Available(T),
    Unavailable { reasons: Vec<ValuationReason> },
}

impl<T> Availability<T> {
    pub fn available(value: T) -> Self {
        Self::Available(value)
    }

    pub fn unavailable(reason: ValuationReason) -> Self {
        Self::Unavailable {
            reasons: vec![reason],
        }
    }

    pub fn unavailable_empty() -> Self {
        Self::Unavailable {
            reasons: Vec::new(),
        }
    }

    pub fn as_ref(&self) -> Option<&T> {
        match self {
            Self::Available(value) => Some(value),
            Self::Unavailable { .. } => None,
        }
    }

    pub fn reasons(&self) -> Vec<ValuationReason> {
        match self {
            Self::Available(_) => Vec::new(),
            Self::Unavailable { reasons } => reasons.clone(),
        }
    }
}

pub(super) fn merge_reasons(
    first: &[ValuationReason],
    second: &[ValuationReason],
    fallback: ValuationReason,
) -> Vec<ValuationReason> {
    let mut reasons = Vec::new();
    reasons.extend_from_slice(first);
    reasons.extend_from_slice(second);
    dedup_reasons(&mut reasons);
    if reasons.is_empty() {
        reasons.push(fallback);
    }
    reasons
}

pub(super) fn merge_many_reasons(
    sources: &[Vec<ValuationReason>],
    fallback: ValuationReason,
) -> Vec<ValuationReason> {
    let mut reasons = Vec::new();
    for source in sources {
        reasons.extend_from_slice(source);
    }
    dedup_reasons(&mut reasons);
    if reasons.is_empty() {
        reasons.push(fallback);
    }
    reasons
}

pub(super) fn unavailable_or_first(
    reasons: Vec<ValuationReason>,
    fallback: ValuationReason,
) -> Availability<Decimal> {
    if reasons.is_empty() {
        Availability::unavailable(fallback)
    } else {
        Availability::Unavailable { reasons }
    }
}

pub(super) fn append_amount_reasons(
    reasons: &mut Vec<ValuationReason>,
    state: &Availability<Decimal>,
) {
    if let Availability::Unavailable {
        reasons: state_reasons,
    } = state
    {
        reasons.extend(state_reasons.clone());
    }
}

pub(super) fn dedup_reasons(reasons: &mut Vec<ValuationReason>) {
    let mut deduped = Vec::with_capacity(reasons.len());
    for reason in reasons.drain(..) {
        if !deduped.contains(&reason) {
            deduped.push(reason);
        }
    }
    *reasons = deduped;
}

#[cfg(test)]
mod tests {
    use super::{dedup_reasons, merge_many_reasons, merge_reasons, ValuationReason};

    #[test]
    fn merge_reasons_deduplicates_in_source_order() {
        let reasons = merge_reasons(
            &[ValuationReason::MissingPrice],
            &[ValuationReason::MissingPrice, ValuationReason::MissingFx],
            ValuationReason::MissingPreviousClose,
        );

        assert_eq!(
            reasons,
            vec![ValuationReason::MissingPrice, ValuationReason::MissingFx]
        );
    }

    #[test]
    fn merge_many_reasons_uses_fallback_when_sources_are_empty() {
        let reasons = merge_many_reasons(&[Vec::new(), Vec::new()], ValuationReason::MissingFx);

        assert_eq!(reasons, vec![ValuationReason::MissingFx]);
    }

    #[test]
    fn dedup_reasons_keeps_first_occurrence() {
        let mut reasons = vec![
            ValuationReason::MissingFx,
            ValuationReason::MissingPrice,
            ValuationReason::MissingFx,
        ];

        dedup_reasons(&mut reasons);

        assert_eq!(
            reasons,
            vec![ValuationReason::MissingFx, ValuationReason::MissingPrice]
        );
    }
}
