use chrono::{DateTime, Local, NaiveDate, Utc};

/// The single source of "today" for every surface that values a portfolio.
#[derive(Clone, Copy, Debug)]
pub enum Clock {
    /// The machine's local calendar date.
    System,
    /// A pinned date, for tests.
    Fixed(NaiveDate),
}

impl Clock {
    pub fn fixed(date: NaiveDate) -> Self {
        Clock::Fixed(date)
    }

    /// Today's date in the app's timezone. Every valuation date, staleness
    /// comparison and refresh window derives from this.
    pub fn today(&self) -> NaiveDate {
        match self {
            Clock::System =>
            {
                #[allow(clippy::disallowed_methods)]
                Local::now().naive_local().date()
            }
            Clock::Fixed(date) => *date,
        }
    }
}

/// The current instant, for timestamps that are points in time rather than
/// calendar dates: log lines, backup file names, refresh run records.
pub fn now_utc() -> DateTime<Utc> {
    #[allow(clippy::disallowed_methods)]
    Utc::now()
}

/// The current instant as an RFC 3339 string, the form stored in the ledger's
/// timestamp columns and in refresh run records.
pub fn now_iso8601() -> String {
    now_utc().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn fixed_clock_returns_its_pinned_date() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 5).expect("valid date");
        assert_eq!(Clock::fixed(date).today(), date);
    }

    #[test]
    fn system_clock_agrees_with_local_calendar_date() {
        let today = Clock::System.today();
        let now = now_utc();
        assert!(
            (now.date_naive() - today).num_days().abs() <= 1,
            "system clock date {today} should be within a day of UTC {}",
            now.date_naive()
        );
    }

    #[test]
    fn now_iso8601_looks_like_rfc3339() {
        let value = now_iso8601();
        chrono::DateTime::parse_from_rfc3339(&value).expect("timestamp parses as RFC3339");
    }
}
