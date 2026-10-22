//! shared metadata and calendar matching for CLI and interactive search

use crate::{data::Session, utils::format_datetime};
use chrono::{Datelike, Days, Local, Months, NaiveDate};
use std::ops::Range;

const DAYS_PER_WEEK: u64 = 7;
const MONTHS_PER_YEAR: u32 = 12;

/// normalize metadata once per search, without copying conversation messages
pub(crate) struct Candidate {
    pub name: String,
    project_path: String,
    session_id: String,
    modified_at: String,
    modified_date: NaiveDate,
}

impl Candidate {
    pub fn new(session: &Session) -> Self {
        Self {
            name: session.name.as_deref().unwrap_or_default().to_lowercase(),
            project_path: session.project_path.to_string_lossy().to_lowercase(),
            session_id: session.session_id.to_lowercase(),
            modified_at: format_datetime(session.last_active_at),
            modified_date: session.last_active_at.with_timezone(&Local).date_naive(),
        }
    }

    pub fn matches(&self, query: &str, period: Option<&Range<NaiveDate>>) -> bool {
        self.name.contains(query)
            || self.project_path.contains(query)
            || self.session_id.contains(query)
            || self.modified_at.contains(query)
            || period.is_some_and(|period| period.contains(&self.modified_date))
    }
}

/// use local calendar dates rather than fixed-hour durations across DST changes
pub(crate) fn calendar_period(query: &str, today: NaiveDate) -> Option<Range<NaiveDate>> {
    match query {
        "today" => Some(today..today.succ_opt()?),
        "yesterday" => Some(today.pred_opt()?..today),
        "this week" | "last week" => {
            let days_since_monday = u64::from(today.weekday().num_days_from_monday());
            let start = today.checked_sub_days(Days::new(days_since_monday))?;
            let week = Days::new(DAYS_PER_WEEK);
            if query == "last week" {
                Some(start.checked_sub_days(week)?..start)
            } else {
                Some(start..start.checked_add_days(week)?)
            }
        }
        "this month" | "last month" | "this year" | "last year" => {
            let mut start = today.with_day(1)?;
            let months = if query.ends_with("year") {
                start = start.with_month(1)?;
                Months::new(MONTHS_PER_YEAR)
            } else {
                Months::new(1)
            };
            if query.starts_with("last ") {
                Some(start.checked_sub_months(months)?..start)
            } else {
                Some(start..start.checked_add_months(months)?)
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    fn candidate(modified: &str) -> Candidate {
        Candidate {
            name: String::new(),
            project_path: String::new(),
            session_id: String::new(),
            modified_at: modified.into(),
            modified_date: date(modified),
        }
    }

    #[test]
    fn calendar_shortcuts_use_complete_local_calendar_periods() {
        let today = date("2026-10-21");
        for (query, start, end) in [
            ("today", "2026-10-21", "2026-10-22"),
            ("yesterday", "2026-10-20", "2026-10-21"),
            ("this week", "2026-10-19", "2026-10-26"),
            ("last week", "2026-10-12", "2026-10-19"),
            ("this month", "2026-10-01", "2026-11-01"),
            ("last month", "2026-09-01", "2026-10-01"),
            ("this year", "2026-01-01", "2027-01-01"),
            ("last year", "2025-01-01", "2026-01-01"),
        ] {
            let period = calendar_period(query, today).unwrap();
            assert_eq!(period, date(start)..date(end), "query: {query}");
            assert!(candidate(start).matches(query, Some(&period)));
            assert!(!candidate(end).matches(query, Some(&period)));
            let before_start = period.start.pred_opt().unwrap().to_string();
            assert!(!candidate(&before_start).matches(query, Some(&period)));
        }
    }

    #[test]
    fn calendar_periods_handle_leap_days_and_month_year_week_boundaries() {
        for (today, query, start, end) in [
            ("2024-03-01", "yesterday", "2024-02-29", "2024-03-01"),
            ("2024-03-31", "last month", "2024-02-01", "2024-03-01"),
            ("2026-01-01", "last month", "2025-12-01", "2026-01-01"),
            ("2026-01-01", "this week", "2025-12-29", "2026-01-05"),
            ("2026-10-19", "last week", "2026-10-12", "2026-10-19"),
            ("2026-10-25", "this week", "2026-10-19", "2026-10-26"),
        ] {
            assert_eq!(
                calendar_period(query, date(today)),
                Some(date(start)..date(end)),
                "today: {today}, query: {query}"
            );
        }
    }

    #[test]
    fn only_whole_calendar_queries_expand_and_literal_matches_still_count() {
        let today = date("2026-10-21");
        for query in ["today notes", "yesterday/", "this", "10-2", "tomorrow"] {
            assert_eq!(calendar_period(query, today), None);
        }
        let mut item = candidate("2025-01-01");
        item.name = "today notes".into();
        let period = calendar_period("today", today).unwrap();
        assert!(item.matches("today", Some(&period)));
        assert!(!item.matches("yesterday", Some(&period)));
    }
}
