use std::sync::{Arc, RwLock};

use chrono::{DateTime, Local, NaiveDate, TimeZone, Utc};

/// The single source of "today" for every surface that values a portfolio.
#[derive(Clone, Debug)]
pub enum Clock {
    /// The machine's local calendar date.
    System,
    /// A pinned date, for tests.
    Fixed(NaiveDate),
    /// A fully pinned instant for lease and timestamp tests.
    FixedInstant(Arc<RwLock<DateTime<Utc>>>),
}

impl Clock {
    pub fn fixed(date: NaiveDate) -> Self {
        Clock::Fixed(date)
    }

    pub fn fixed_instant(instant: DateTime<Utc>) -> Self {
        Clock::FixedInstant(Arc::new(RwLock::new(instant)))
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
            Clock::FixedInstant(instant) => instant
                .read()
                .expect("fixed clock lock should not be poisoned")
                .with_timezone(&Local)
                .date_naive(),
        }
    }

    pub fn now_utc(&self) -> DateTime<Utc> {
        match self {
            Clock::System => now_utc(),
            Clock::Fixed(date) => {
                Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0).expect("midnight"))
            }
            Clock::FixedInstant(instant) => *instant
                .read()
                .expect("fixed clock lock should not be poisoned"),
        }
    }

    /// Convert a stored instant using the same calendar as today(). Date-only
    /// test clocks use UTC for their synthetic midnight timestamps.
    pub fn local_date(&self, instant: DateTime<Utc>) -> NaiveDate {
        match self {
            Clock::Fixed(_) => instant.date_naive(),
            Clock::System | Clock::FixedInstant(_) => instant.with_timezone(&Local).date_naive(),
        }
    }

    #[cfg(test)]
    pub(crate) fn set_instant(&self, instant: DateTime<Utc>) {
        match self {
            Clock::FixedInstant(current) => {
                *current
                    .write()
                    .expect("fixed clock lock should not be poisoned") = instant;
            }
            _ => panic!("set_instant requires Clock::FixedInstant"),
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
        let clock = Clock::fixed(date);
        assert_eq!(clock.local_date(clock.now_utc()), clock.today());
        assert_eq!(
            clock.local_date(clock.now_utc() - chrono::Duration::days(1)),
            date - chrono::Duration::days(1)
        );
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
