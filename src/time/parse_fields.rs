//! Shared parser for the `p:` / `due:` / `est:` inline fields used by both `task add`
//! and `task edit`.
//!
//! Centralizing this here gives us:
//! - case-insensitive field prefixes (`P:` / `Due:` / `EST:` all work)
//! - duplicate-field detection (`p:a p:b` errors instead of silently keeping the last)
//! - trimming + non-empty validation on the text portion
//! - one place to improve error messages (e.g. distinguishing empty priority from a
//!   priority typo)

use crate::error::{Error, Result};
use crate::model::Priority;
use crate::time::{parse_due, parse_duration};
use chrono::{DateTime, Local};

#[derive(Debug)]
pub struct ParsedFields {
    /// Trimmed task text, joined from positional args with single spaces. `None` when
    /// the caller didn't supply any text tokens (used by `edit` to leave text alone).
    pub text: Option<String>,
    pub priority: Option<Priority>,
    pub due: Option<DateTime<Local>>,
    pub est_secs: Option<i64>,
}

/// Recognise the three field prefixes, case-insensitively. Returns the suffix or
/// `None` if `arg` is a plain text token.
fn strip_field_prefix<'a>(arg: &'a str) -> Option<(Field, &'a str)> {
    // We can't use `to_lowercase` on the whole arg because we need to keep the value
    // unchanged (priority values *are* case-insensitive, but due:/est: values may be
    // case-sensitive — e.g. ISO month-day "Jun15"). So check the first 3-4 bytes.
    let (head, rest) = match arg.find(':') {
        Some(idx) => (&arg[..idx], &arg[idx + 1..]),
        None => return None,
    };
    let head_lower = head.to_ascii_lowercase();
    match head_lower.as_str() {
        "p" => Some((Field::Priority, rest)),
        "due" => Some((Field::Due, rest)),
        "est" => Some((Field::Est, rest)),
        _ => None,
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Field {
    Priority,
    Due,
    Est,
}

impl Field {
    fn name(self) -> &'static str {
        match self {
            Field::Priority => "p:",
            Field::Due => "due:",
            Field::Est => "est:",
        }
    }
}

pub fn parse_task_fields(args: &[String], now_local: DateTime<Local>) -> Result<ParsedFields> {
    let mut text_parts: Vec<&str> = Vec::new();
    let mut priority: Option<Priority> = None;
    let mut due: Option<DateTime<Local>> = None;
    let mut est_secs: Option<i64> = None;

    for arg in args {
        let Some((field, rest)) = strip_field_prefix(arg) else {
            text_parts.push(arg);
            continue;
        };
        match field {
            Field::Priority => {
                if priority.is_some() {
                    return Err(Error::Parse(format!(
                        "duplicate {} field",
                        field.name()
                    )));
                }
                priority = Some(parse_priority(rest)?);
            }
            Field::Due => {
                if due.is_some() {
                    return Err(Error::Parse(format!(
                        "duplicate {} field",
                        field.name()
                    )));
                }
                let value = rest.trim();
                if value.is_empty() {
                    return Err(Error::Parse("due: value is required".into()));
                }
                due = Some(parse_due(value, now_local)?);
            }
            Field::Est => {
                if est_secs.is_some() {
                    return Err(Error::Parse(format!(
                        "duplicate {} field",
                        field.name()
                    )));
                }
                let value = rest.trim();
                if value.is_empty() {
                    return Err(Error::Parse("est: value is required".into()));
                }
                est_secs = Some(parse_duration(value)?.num_seconds());
            }
        }
    }

    let text = if text_parts.is_empty() {
        None
    } else {
        let joined = text_parts.join(" ");
        let trimmed = joined.trim();
        if trimmed.is_empty() {
            return Err(Error::Parse("task text must not be empty".into()));
        }
        Some(trimmed.to_string())
    };

    Ok(ParsedFields {
        text,
        priority,
        due,
        est_secs,
    })
}

fn parse_priority(value: &str) -> Result<Priority> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(Error::Parse("p: value is required (A, B, or C)".into()));
    }
    // Reject `p:a:b` and similar — the colon-split caller already grabbed `a:b text`
    // as the value, which produces confusing downstream errors.
    if trimmed.contains(':') || trimmed.contains(char::is_whitespace) {
        return Err(Error::Parse(format!(
            "invalid priority '{trimmed}', expected A, B, or C"
        )));
    }
    trimmed.parse().map_err(Error::Parse)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Local> {
        Local
            .from_local_datetime(
                &chrono::NaiveDate::from_ymd_opt(2026, 5, 17)
                    .unwrap()
                    .and_hms_opt(10, 0, 0)
                    .unwrap(),
            )
            .unwrap()
    }

    fn parse(args: &[&str]) -> Result<ParsedFields> {
        let v: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        parse_task_fields(&v, now())
    }

    #[test]
    fn text_only() {
        let p = parse(&["Buy", "milk"]).unwrap();
        assert_eq!(p.text.as_deref(), Some("Buy milk"));
    }

    #[test]
    fn empty_string_text_rejected() {
        assert!(parse(&[""]).is_err());
    }

    #[test]
    fn whitespace_only_text_rejected() {
        assert!(parse(&["   "]).is_err());
    }

    #[test]
    fn trailing_space_trimmed() {
        // " x " comes in as one positional arg; joined+trimmed should be "x".
        let p = parse(&[" x "]).unwrap();
        assert_eq!(p.text.as_deref(), Some("x"));
    }

    #[test]
    fn priority_uppercase_prefix() {
        let p = parse(&["task", "P:a"]).unwrap();
        assert_eq!(p.priority, Some(Priority::A));
    }

    #[test]
    fn due_mixed_case_prefix() {
        let p = parse(&["task", "Due:tomorrow"]).unwrap();
        assert!(p.due.is_some());
    }

    #[test]
    fn est_uppercase_prefix() {
        let p = parse(&["task", "EST:1h"]).unwrap();
        assert_eq!(p.est_secs, Some(3600));
    }

    #[test]
    fn duplicate_priority_rejected() {
        assert!(parse(&["task", "p:a", "p:b"]).is_err());
    }

    #[test]
    fn duplicate_due_rejected() {
        assert!(parse(&["task", "due:tomorrow", "due:friday"]).is_err());
    }

    #[test]
    fn duplicate_est_rejected() {
        assert!(parse(&["task", "est:1h", "est:30m"]).is_err());
    }

    #[test]
    fn empty_priority_value_clear_error() {
        let err = parse(&["task", "p:"]).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("required"), "got: {msg}");
    }

    #[test]
    fn priority_with_colon_is_rejected() {
        let err = parse(&["task", "p:a:b"]).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("invalid priority"), "got: {msg}");
    }

    #[test]
    fn negative_est_rejected() {
        assert!(parse(&["task", "est:-1h"]).is_err());
    }

    #[test]
    fn no_text_returns_none_text() {
        let p = parse(&["p:a"]).unwrap();
        assert!(p.text.is_none());
        assert_eq!(p.priority, Some(Priority::A));
    }
}
