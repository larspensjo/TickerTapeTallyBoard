use std::collections::BTreeMap;

use super::PriceCandidate;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderCode(String);

impl ProviderCode {
    pub(crate) fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Resolves date-ascending price series ordered from highest to lowest
/// precedence. The first series supplying a date wins that date.
pub fn resolve_price_series(series_by_precedence: &[&[PriceCandidate]]) -> Vec<PriceCandidate> {
    let mut resolved = BTreeMap::new();
    for series in series_by_precedence {
        for candidate in *series {
            resolved
                .entry(candidate.date)
                .or_insert_with(|| candidate.clone());
        }
    }
    resolved.into_values().collect()
}

/// Picks the newest candidate; when dates tie, the earlier precedence slot wins.
pub fn pick_latest_on_or_before(
    candidates_by_precedence: &[Option<PriceCandidate>],
) -> Option<PriceCandidate> {
    candidates_by_precedence
        .iter()
        .flatten()
        .fold(None, |selected, candidate| match selected {
            Some(current) if current.date >= candidate.date => Some(current),
            _ => Some(candidate.clone()),
        })
}

/// Picks the newest candidate; when dates tie, the earlier precedence slot wins.
pub fn pick_previous_before(
    candidates_by_precedence: &[Option<PriceCandidate>],
) -> Option<PriceCandidate> {
    pick_latest_on_or_before(candidates_by_precedence)
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use rust_decimal_macros::dec;

    use super::*;

    fn candidate(day: u32, source: &str) -> PriceCandidate {
        PriceCandidate {
            date: NaiveDate::from_ymd_opt(2026, 6, day).expect("valid date"),
            close: dec!(100),
            currency: "SEK".to_owned(),
            source: ProviderCode::new(source),
        }
    }

    #[test]
    fn higher_precedence_wins_shared_date_and_lower_fills_gap() {
        let high = [candidate(10, "HIGH")];
        let low = [candidate(10, "LOW"), candidate(11, "LOW")];

        let resolved = resolve_price_series(&[&high, &low]);

        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].source.as_str(), "HIGH");
        assert_eq!(resolved[1].source.as_str(), "LOW");
    }

    #[test]
    fn resolution_handles_single_empty_and_interleaved_inputs() {
        let single = [candidate(10, "ONE")];
        assert_eq!(resolve_price_series(&[&single]), single);
        assert!(resolve_price_series(&[]).is_empty());

        let high = [candidate(11, "HIGH"), candidate(13, "HIGH")];
        let low = [candidate(10, "LOW"), candidate(12, "LOW")];
        let dates: Vec<_> = resolve_price_series(&[&high, &low])
            .into_iter()
            .map(|value| value.date)
            .collect();
        assert_eq!(
            dates,
            vec![
                NaiveDate::from_ymd_opt(2026, 6, 10).expect("valid date"),
                NaiveDate::from_ymd_opt(2026, 6, 11).expect("valid date"),
                NaiveDate::from_ymd_opt(2026, 6, 12).expect("valid date"),
                NaiveDate::from_ymd_opt(2026, 6, 13).expect("valid date"),
            ]
        );
    }

    #[test]
    fn picks_newest_candidate_and_uses_precedence_for_ties() {
        let high = candidate(10, "HIGH");
        let low_newer = candidate(11, "LOW");
        assert_eq!(
            pick_latest_on_or_before(&[Some(high.clone()), Some(low_newer.clone())]),
            Some(low_newer)
        );

        let low_same_day = candidate(10, "LOW");
        assert_eq!(
            pick_latest_on_or_before(&[Some(high.clone()), Some(low_same_day.clone())]),
            Some(high.clone())
        );
        assert_eq!(
            pick_previous_before(&[Some(high.clone()), Some(low_same_day)]),
            Some(high)
        );
    }
}
