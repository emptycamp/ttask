//! Working-days arithmetic for the GC sweep. A "working day" is a Mon–Fri local
//! calendar day. Weekends (Sat/Sun) are skipped so a task created Friday isn't
//! considered "2 work days old" by Sunday.

use chrono::{DateTime, Datelike, Duration, Local, Utc, Weekday};

/// Whole working days elapsed between `start` and `end` in the local timezone.
/// Returns 0 when `end <= start`. We compare on calendar boundaries, so a task
/// touched at 23:59 still counts as "0 work days old" the next morning until the
/// calendar rolls past one more weekday.
pub fn work_days_between(start: DateTime<Utc>, end: DateTime<Utc>) -> i64 {
    let start_local: DateTime<Local> = start.into();
    let end_local: DateTime<Local> = end.into();
    let start_date = start_local.date_naive();
    let end_date = end_local.date_naive();
    if end_date <= start_date {
        return 0;
    }
    let mut count: i64 = 0;
    let mut d = start_date;
    while d < end_date {
        d += Duration::days(1);
        if !is_weekend(d.weekday()) {
            count += 1;
        }
    }
    count
}

fn is_weekend(w: Weekday) -> bool {
    matches!(w, Weekday::Sat | Weekday::Sun)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(year: i32, month: u32, day: u32) -> DateTime<Utc> {
        Local
            .with_ymd_and_hms(year, month, day, 12, 0, 0)
            .single()
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn zero_when_same_day() {
        let d = at(2026, 5, 18); // Monday
        assert_eq!(work_days_between(d, d), 0);
    }

    #[test]
    fn one_work_day_mon_to_tue() {
        let mon = at(2026, 5, 18);
        let tue = at(2026, 5, 19);
        assert_eq!(work_days_between(mon, tue), 1);
    }

    #[test]
    fn fri_to_mon_is_one_work_day() {
        // Fri 22 May → Mon 25 May: only Monday is counted (Sat/Sun skipped).
        let fri = at(2026, 5, 22);
        let mon = at(2026, 5, 25);
        assert_eq!(work_days_between(fri, mon), 1);
    }

    #[test]
    fn fri_to_wed_is_three_work_days() {
        // Fri → Sat (skip) → Sun (skip) → Mon (+1) → Tue (+2) → Wed (+3).
        let fri = at(2026, 5, 22);
        let wed = at(2026, 5, 27);
        assert_eq!(work_days_between(fri, wed), 3);
    }

    #[test]
    fn mon_to_next_mon_is_five_work_days() {
        // One full work week: Mon → Mon spans 5 weekdays (Tue..Fri + next Mon).
        let mon1 = at(2026, 5, 18);
        let mon2 = at(2026, 5, 25);
        assert_eq!(work_days_between(mon1, mon2), 5);
    }

    #[test]
    fn negative_range_returns_zero() {
        let mon = at(2026, 5, 18);
        let tue = at(2026, 5, 19);
        assert_eq!(work_days_between(tue, mon), 0);
    }

    #[test]
    fn weekend_only_range_is_zero() {
        let sat = at(2026, 5, 23);
        let sun = at(2026, 5, 24);
        assert_eq!(work_days_between(sat, sun), 0);
    }
}
