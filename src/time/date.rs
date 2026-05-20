use crate::error::{Error, Result};
use crate::time::duration::parse_duration;
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone, Weekday};

/// Result of trying one of the parser strategies. `Match(Ok)` / `Match(Err)` means the
/// input *looked like* this shape (e.g. starts with a month name, or is `YYYY-MM-DD`)
/// — so we should report the failure instead of silently falling through to the next
/// strategy and emitting a misleading duration-parse error.
enum TryResult {
    Match(Result<DateTime<Local>>),
    NoMatch,
}

pub fn parse_due(s: &str, now: DateTime<Local>) -> Result<DateTime<Local>> {
    let s = s.trim().to_lowercase();

    if s.is_empty() {
        return Err(Error::Parse("due value is required".into()));
    }

    if s == "now" {
        return Ok(now);
    }

    // Mirror of how relative time is displayed ("in 5m"). Treat the prefix as syntactic
    // sugar so what the editor seeds into the Due field round-trips cleanly.
    let s: String = s.strip_prefix("in ").unwrap_or(&s).trim().to_string();

    if let Some(dt) = try_keyword(&s, now) {
        return validate(dt, now);
    }

    if let TryResult::Match(r) = try_iso(&s) {
        return r.and_then(|dt| validate(dt, now));
    }

    if let Some(dt) = try_weekday(&s, now) {
        return validate(dt, now);
    }

    if let TryResult::Match(r) = try_month_day(&s, now) {
        return r.and_then(|dt| validate(dt, now));
    }

    let dur = parse_duration(&s)?;
    validate(now + dur, now)
}

fn try_iso(s: &str) -> TryResult {
    // "YYYY-MM-DD HH:MM" or "YYYY-MM-DDTHH:MM" or "YYYY-MM-DD"
    let normalized = s.replacen('t', " ", 1);
    let parts: Vec<&str> = normalized.splitn(2, ' ').collect();
    let date_part = parts[0];
    let time_part = parts.get(1).copied().unwrap_or("09:00");

    let date_pieces: Vec<&str> = date_part.split('-').collect();
    if date_pieces.len() != 3 {
        return TryResult::NoMatch;
    }
    // Once we know it's a YYYY-MM-DD shape, any failure beyond this point is an
    // invalid-date error rather than "not an ISO date" — falling through to the
    // duration parser would have buried the real problem.
    let parse_ymd = || -> Result<DateTime<Local>> {
        let year: i32 = date_pieces[0]
            .parse()
            .map_err(|_| Error::Parse(format!("invalid year '{}'", date_pieces[0])))?;
        let month: u32 = date_pieces[1]
            .parse()
            .map_err(|_| Error::Parse(format!("invalid month '{}'", date_pieces[1])))?;
        let day: u32 = date_pieces[2]
            .parse()
            .map_err(|_| Error::Parse(format!("invalid day '{}'", date_pieces[2])))?;

        let time_pieces: Vec<&str> = time_part.split(':').collect();
        let hour: u32 = time_pieces
            .first()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| Error::Parse(format!("invalid time '{time_part}'")))?;
        let minute: u32 = time_pieces.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);

        let date = NaiveDate::from_ymd_opt(year, month, day)
            .ok_or_else(|| Error::Parse(format!("invalid date '{year}-{month:02}-{day:02}'")))?;
        let naive = date
            .and_hms_opt(hour, minute, 0)
            .ok_or_else(|| Error::Parse(format!("invalid time '{hour:02}:{minute:02}'")))?;
        Local
            .from_local_datetime(&naive)
            .single()
            .ok_or_else(|| Error::Parse(format!("ambiguous local time '{naive}'")))
    };
    TryResult::Match(parse_ymd())
}

fn try_keyword(s: &str, now: DateTime<Local>) -> Option<DateTime<Local>> {
    match s {
        // "today" lands at end-of-day local (5pm) so that adding a task in the
        // afternoon doesn't immediately produce a due-time that's already past.
        // QA H4: previously today/tonight both resolved to today 09:00, which became
        // overdue any time a user added a task after morning.
        "today" => Some(future_or_tomorrow(now, 17, 0)),
        // "tonight" is the evening time users actually expect (8pm). Same future-or-
        // tomorrow guarantee.
        "tonight" => Some(future_or_tomorrow(now, 20, 0)),
        "tomorrow" => Some(local_at(now + Duration::days(1), 9, 0)),
        _ => None,
    }
}

/// Build "today at hh:mm" — but if that time has already passed locally, roll forward
/// to the same time tomorrow. Used by the `today` / `tonight` keywords to guarantee
/// the resulting due-time is always in the future.
fn future_or_tomorrow(now: DateTime<Local>, hour: u32, min: u32) -> DateTime<Local> {
    let candidate = local_at(now, hour, min);
    if candidate > now {
        candidate
    } else {
        local_at(now + Duration::days(1), hour, min)
    }
}

fn local_at(base: DateTime<Local>, hour: u32, min: u32) -> DateTime<Local> {
    base.date_naive()
        .and_hms_opt(hour, min, 0)
        .and_then(|ndt| Local.from_local_datetime(&ndt).single())
        .unwrap_or(base)
}

fn try_weekday(s: &str, now: DateTime<Local>) -> Option<DateTime<Local>> {
    let target = parse_weekday(s)?;
    let today = now.weekday();
    let days_ahead = days_until(today, target);
    let date = now + Duration::days(days_ahead as i64);
    Some(local_at(date, 9, 0))
}

fn parse_weekday(s: &str) -> Option<Weekday> {
    match s {
        "mon" | "monday" => Some(Weekday::Mon),
        "tue" | "tuesday" => Some(Weekday::Tue),
        "wed" | "wednesday" => Some(Weekday::Wed),
        "thu" | "thursday" => Some(Weekday::Thu),
        "fri" | "friday" => Some(Weekday::Fri),
        "sat" | "saturday" => Some(Weekday::Sat),
        "sun" | "sunday" => Some(Weekday::Sun),
        _ => None,
    }
}

fn days_until(from: Weekday, to: Weekday) -> u32 {
    let from_num = from.num_days_from_monday();
    let to_num = to.num_days_from_monday();
    if to_num > from_num {
        to_num - from_num
    } else {
        7 - from_num + to_num
    }
}

fn try_month_day(s: &str, now: DateTime<Local>) -> TryResult {
    let months = [
        ("january", 1),
        ("jan", 1),
        ("february", 2),
        ("feb", 2),
        ("march", 3),
        ("mar", 3),
        ("april", 4),
        ("apr", 4),
        ("may", 5),
        ("june", 6),
        ("jun", 6),
        ("july", 7),
        ("jul", 7),
        ("august", 8),
        ("aug", 8),
        ("september", 9),
        ("sep", 9),
        ("october", 10),
        ("oct", 10),
        ("november", 11),
        ("nov", 11),
        ("december", 12),
        ("dec", 12),
    ];

    for (name, month) in &months {
        if let Some(rest) = s.strip_prefix(name) {
            let rest = rest.trim_start_matches(|c: char| c == ' ' || c == '-' || c == '/');
            if rest.is_empty() {
                // M1: "jan" alone is a recognized month — emit a clear error rather
                // than falling through to "unknown duration unit".
                return TryResult::Match(Err(Error::Parse(format!(
                    "expected day after '{name}', e.g. '{name}15'"
                ))));
            }
            let day: u32 = match rest.parse() {
                Ok(d) => d,
                Err(_) => {
                    return TryResult::Match(Err(Error::Parse(format!(
                        "invalid day '{rest}' after '{name}'"
                    ))))
                }
            };
            let year = now.year();
            let result = NaiveDate::from_ymd_opt(year, *month, day)
                .and_then(|d| d.and_hms_opt(9, 0, 0))
                .and_then(|ndt| Local.from_local_datetime(&ndt).single());
            return match result {
                Some(dt) if dt > now => TryResult::Match(Ok(dt)),
                Some(_) => {
                    // Same calendar position has already passed this year — try next.
                    let next = NaiveDate::from_ymd_opt(year + 1, *month, day)
                        .and_then(|d| d.and_hms_opt(9, 0, 0))
                        .and_then(|ndt| Local.from_local_datetime(&ndt).single())
                        .ok_or_else(|| {
                            Error::Parse(format!(
                                "invalid date '{name} {day}' next year"
                            ))
                        });
                    TryResult::Match(next)
                }
                None => TryResult::Match(Err(Error::Parse(format!(
                    "invalid date '{name} {day}'"
                )))),
            };
        }
    }
    TryResult::NoMatch
}

fn validate(dt: DateTime<Local>, now: DateTime<Local>) -> Result<DateTime<Local>> {
    let limit = now + Duration::days(366);
    if dt > limit {
        return Err(Error::Parse(
            "due date must be within 12 months".into(),
        ));
    }
    Ok(dt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Timelike};

    fn make_now(year: i32, month: u32, day: u32) -> DateTime<Local> {
        Local
            .from_local_datetime(
                &chrono::NaiveDate::from_ymd_opt(year, month, day)
                    .unwrap()
                    .and_hms_opt(10, 0, 0)
                    .unwrap(),
            )
            .unwrap()
    }

    #[test]
    fn keyword_today_resolves_to_end_of_today_when_before_5pm() {
        // make_now is 10:00 — today 17:00 is still in the future.
        let now = make_now(2026, 5, 17);
        let due = parse_due("today", now).unwrap();
        assert_eq!(due.date_naive(), now.date_naive());
        assert_eq!(due.hour(), 17);
        assert!(due > now, "today must always be in the future");
    }

    #[test]
    fn keyword_today_rolls_to_tomorrow_after_5pm() {
        // H4 regression: 23:24 was producing a same-day 09:00, i.e. 14h in the past.
        let now = Local
            .from_local_datetime(
                &chrono::NaiveDate::from_ymd_opt(2026, 5, 17)
                    .unwrap()
                    .and_hms_opt(23, 24, 0)
                    .unwrap(),
            )
            .unwrap();
        let due = parse_due("today", now).unwrap();
        assert!(due > now, "due must be in the future, got {due} (now={now})");
    }

    #[test]
    fn keyword_tonight_resolves_to_8pm() {
        let now = make_now(2026, 5, 17);
        let due = parse_due("tonight", now).unwrap();
        assert_eq!(due.hour(), 20);
        assert!(due > now);
    }

    #[test]
    fn keyword_tonight_rolls_to_tomorrow_when_late() {
        let now = Local
            .from_local_datetime(
                &chrono::NaiveDate::from_ymd_opt(2026, 5, 17)
                    .unwrap()
                    .and_hms_opt(22, 30, 0)
                    .unwrap(),
            )
            .unwrap();
        let due = parse_due("tonight", now).unwrap();
        assert!(due > now);
    }

    #[test]
    fn keyword_tomorrow() {
        let now = make_now(2026, 5, 17);
        let due = parse_due("tomorrow", now).unwrap();
        assert_eq!(due.date_naive(), (now + Duration::days(1)).date_naive());
    }

    #[test]
    fn weekday_next_wednesday_from_wednesday() {
        // On a Wednesday, "wed" returns next Wednesday (7 days)
        let now = make_now(2026, 5, 20); // May 20, 2026 is a Wednesday
        let due = parse_due("wed", now).unwrap();
        let expected = make_now(2026, 5, 27);
        assert_eq!(due.date_naive(), expected.date_naive());
    }

    #[test]
    fn weekday_friday_from_monday() {
        let now = make_now(2026, 5, 18); // Monday
        let due = parse_due("fri", now).unwrap();
        let expected = make_now(2026, 5, 22);
        assert_eq!(due.date_naive(), expected.date_naive());
    }

    #[test]
    fn weekday_full_name() {
        let now = make_now(2026, 5, 18); // Monday
        let due = parse_due("friday", now).unwrap();
        let expected = make_now(2026, 5, 22);
        assert_eq!(due.date_naive(), expected.date_naive());
    }

    #[test]
    fn month_day_future_same_year() {
        let now = make_now(2026, 5, 17);
        let due = parse_due("jun1", now).unwrap();
        assert_eq!(due.year(), 2026);
        assert_eq!(due.month(), 6);
        assert_eq!(due.day(), 1);
    }

    #[test]
    fn month_day_past_same_year_wraps_to_next_year() {
        let now = make_now(2026, 5, 17);
        let due = parse_due("mar2", now).unwrap();
        assert_eq!(due.year(), 2027);
        assert_eq!(due.month(), 3);
        assert_eq!(due.day(), 2);
    }

    #[test]
    fn month_full_name_with_space() {
        let now = make_now(2026, 5, 17);
        let due = parse_due("june 1", now).unwrap();
        assert_eq!(due.month(), 6);
        assert_eq!(due.day(), 1);
    }

    #[test]
    fn duration_fallthrough() {
        let now = make_now(2026, 5, 17);
        let due = parse_due("2h", now).unwrap();
        let expected = now + Duration::hours(2);
        assert_eq!(due, expected);
    }

    #[test]
    fn beyond_12_months_returns_error() {
        let now = make_now(2026, 5, 17);
        assert!(parse_due("400d", now).is_err());
    }

    // M1: error messages for shapes-that-look-like-a-date should mention the date
    // problem, not bottom out in "unknown duration unit".
    #[test]
    fn iso_date_with_invalid_month_reports_date_error() {
        let now = make_now(2026, 5, 17);
        let err = parse_due("2026-13-45", now).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("invalid date"), "got: {msg}");
    }

    #[test]
    fn month_day_invalid_day_reports_date_error() {
        let now = make_now(2026, 5, 17);
        let err = parse_due("feb29", now).unwrap_err();
        let msg = format!("{err}");
        // 2026 is not a leap year — Feb 29 is invalid this year, valid next.
        // The parser rolls forward to 2027… wait, 2028 is the next leap year.
        // So this should be an error. But the parser tries year+1 (2027), which also
        // isn't a leap year. So the error message should still mention the date.
        assert!(
            msg.contains("invalid") || msg.contains("date"),
            "got: {msg}"
        );
    }

    #[test]
    fn empty_due_value_returns_error() {
        let now = make_now(2026, 5, 17);
        assert!(parse_due("", now).is_err());
    }
}
