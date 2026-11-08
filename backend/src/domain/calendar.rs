use chrono::{Datelike, Duration, Months, NaiveDate};

pub fn month_end(date: NaiveDate) -> Option<NaiveDate> {
    date.with_day(1)?
        .checked_add_months(Months::new(1))?
        .checked_sub_signed(Duration::days(1))
}
