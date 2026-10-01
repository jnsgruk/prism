use time::Date;

use super::PeriodType;

/// Determine period boundaries for a given date and period type.
#[allow(clippy::expect_used)] // date arithmetic on known-valid values (day 1, known months)
pub fn period_boundaries(reference_date: Date, period_type: PeriodType) -> (Date, Date) {
    match period_type {
        PeriodType::Week => {
            let weekday = reference_date.weekday().number_days_from_monday();
            let start = reference_date - time::Duration::days(i64::from(weekday));
            let end = start + time::Duration::days(6);
            (start, end)
        }
        PeriodType::Month => {
            let start = reference_date.replace_day(1).expect("day 1 always valid");
            let next_month = if reference_date.month() == time::Month::December {
                start
                    .replace_year(reference_date.year() + 1)
                    .expect("year valid")
                    .replace_month(time::Month::January)
                    .expect("month valid")
            } else {
                start
                    .replace_month(reference_date.month().next())
                    .expect("next month valid")
            };
            let end = next_month - time::Duration::days(1);
            (start, end)
        }
        PeriodType::Quarter => {
            let quarter_start_month = match reference_date.month() {
                time::Month::January | time::Month::February | time::Month::March => {
                    time::Month::January
                }
                time::Month::April | time::Month::May | time::Month::June => time::Month::April,
                time::Month::July | time::Month::August | time::Month::September => {
                    time::Month::July
                }
                _ => time::Month::October,
            };
            let start = Date::from_calendar_date(reference_date.year(), quarter_start_month, 1)
                .expect("quarter start valid");
            let end_month = match quarter_start_month {
                time::Month::January => time::Month::March,
                time::Month::April => time::Month::June,
                time::Month::July => time::Month::September,
                _ => time::Month::December,
            };
            let next_quarter = if end_month == time::Month::December {
                Date::from_calendar_date(reference_date.year() + 1, time::Month::January, 1)
                    .expect("next year valid")
            } else {
                Date::from_calendar_date(reference_date.year(), end_month.next(), 1)
                    .expect("next quarter valid")
            };
            let end = next_quarter - time::Duration::days(1);
            (start, end)
        }
    }
}
