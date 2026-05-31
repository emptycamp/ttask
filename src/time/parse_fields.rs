//! Shared parser for the `c:` / `ord:` / `est:` inline fields used by both `task add`
//! and `task edit`. The `c:` prefix is the category (A/B/C); we also accept `p:`
//! as a legacy alias so muscle memory keeps working after the rename.

use crate::error::{Error, Result};
use crate::model::Category;
use crate::time::parse_duration;

#[derive(Debug)]
pub struct ParsedFields {
    /// Trimmed task text, joined from positional args with single spaces. `None` when
    /// the caller didn't supply any text tokens (used by `edit` to leave text alone).
    pub text: Option<String>,
    pub category: Option<Category>,
    pub ord: Option<u32>,
    pub est_secs: Option<i64>,
}

fn strip_field_prefix(arg: &str) -> Option<(Field, &str)> {
    let (head, rest) = match arg.find(':') {
        Some(idx) => (&arg[..idx], &arg[idx + 1..]),
        None => return None,
    };
    let head_lower = head.to_ascii_lowercase();
    match head_lower.as_str() {
        // `p:` is the pre-rename alias — kept so existing scripts and notes
        // don't break. `c:` and `cat:` / `category:` are the current spellings.
        "c" | "cat" | "category" | "p" => Some((Field::Category, rest)),
        "ord" | "order" => Some((Field::Ord, rest)),
        "est" => Some((Field::Est, rest)),
        _ => None,
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Field {
    Category,
    Ord,
    Est,
}

impl Field {
    fn name(self) -> &'static str {
        match self {
            Field::Category => "c:",
            Field::Ord => "ord:",
            Field::Est => "est:",
        }
    }
}

pub fn parse_task_fields(args: &[String]) -> Result<ParsedFields> {
    let mut text_parts: Vec<&str> = Vec::new();
    let mut category: Option<Category> = None;
    let mut ord: Option<u32> = None;
    let mut est_secs: Option<i64> = None;

    for arg in args {
        let Some((field, rest)) = strip_field_prefix(arg) else {
            text_parts.push(arg);
            continue;
        };
        match field {
            Field::Category => {
                if category.is_some() {
                    return Err(Error::Parse(format!("duplicate {} field", field.name())));
                }
                category = Some(parse_category(rest)?);
            }
            Field::Ord => {
                if ord.is_some() {
                    return Err(Error::Parse(format!("duplicate {} field", field.name())));
                }
                let value = rest.trim();
                if value.is_empty() {
                    return Err(Error::Parse("ord: value is required".into()));
                }
                let n: u32 = value.parse().map_err(|_| {
                    Error::Parse(format!(
                        "invalid ord '{value}', expected a positive integer"
                    ))
                })?;
                if n == 0 {
                    return Err(Error::Parse("ord must be >= 1".into()));
                }
                ord = Some(n);
            }
            Field::Est => {
                if est_secs.is_some() {
                    return Err(Error::Parse(format!("duplicate {} field", field.name())));
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

    // A bare duration token at the start or end of the text (e.g. `Buy milk 30m`)
    // is treated as the estimate — same shorthand the edit TUI uses. An explicit
    // `est:` always wins, so only fill in the estimate when it wasn't set.
    let (text, est_secs) = match (text, est_secs) {
        (Some(t), None) => match split_estimate(&t) {
            (stripped, Some(secs)) => (Some(stripped), Some(secs)),
            (_, None) => (Some(t), None),
        },
        (t, e) => (t, e),
    };

    Ok(ParsedFields {
        text,
        category,
        ord,
        est_secs,
    })
}

/// Pull a duration token off the start or end of `text`, if one is present.
/// Returns the remaining text and the matched estimate in seconds.
///
/// A token only counts when it contains both a digit and a unit letter, so
/// `30m` / `4.5h` match but a bare number (`5`) or a plain word (`milk`) stay in
/// the text. A trailing token wins over a leading one. Used by both `task add`
/// and the edit TUI so the shorthand behaves identically in both.
pub fn split_estimate(text: &str) -> (String, Option<i64>) {
    let trimmed = text.trim();
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    // Need at least one text token besides the duration — never reduce the text
    // to empty just because the whole input looks like a duration.
    if tokens.len() < 2 {
        return (trimmed.to_string(), None);
    }
    if let Some(secs) = duration_token_secs(tokens[tokens.len() - 1]) {
        return (tokens[..tokens.len() - 1].join(" "), Some(secs));
    }
    if let Some(secs) = duration_token_secs(tokens[0]) {
        return (tokens[1..].join(" "), Some(secs));
    }
    (trimmed.to_string(), None)
}

fn duration_token_secs(tok: &str) -> Option<i64> {
    let has_digit = tok.chars().any(|c| c.is_ascii_digit());
    let has_unit = tok.chars().any(|c| c.is_ascii_alphabetic());
    if !has_digit || !has_unit {
        return None;
    }
    parse_duration(tok).ok().map(|d| d.num_seconds())
}

fn parse_category(value: &str) -> Result<Category> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(Error::Parse("c: value is required (A, B, or C)".into()));
    }
    if trimmed.contains(':') || trimmed.contains(char::is_whitespace) {
        return Err(Error::Parse(format!(
            "invalid category '{trimmed}', expected A, B, or C"
        )));
    }
    trimmed.parse().map_err(Error::Parse)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<ParsedFields> {
        let v: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        parse_task_fields(&v)
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
    fn category_with_c_prefix() {
        let p = parse(&["task", "c:a"]).unwrap();
        assert_eq!(p.category, Some(Category::A));
    }

    #[test]
    fn category_uppercase_prefix() {
        let p = parse(&["task", "C:a"]).unwrap();
        assert_eq!(p.category, Some(Category::A));
    }

    #[test]
    fn category_with_full_word_prefix() {
        let p = parse(&["task", "category:b"]).unwrap();
        assert_eq!(p.category, Some(Category::B));
    }

    #[test]
    fn category_p_alias_still_accepted() {
        // `p:` was the prior spelling. Keep it working to avoid breaking
        // muscle memory and existing notes/scripts.
        let p = parse(&["task", "p:c"]).unwrap();
        assert_eq!(p.category, Some(Category::C));
    }

    #[test]
    fn est_uppercase_prefix() {
        let p = parse(&["task", "EST:1h"]).unwrap();
        assert_eq!(p.est_secs, Some(3600));
    }

    #[test]
    fn ord_parses_positive_integer() {
        let p = parse(&["task", "ord:3"]).unwrap();
        assert_eq!(p.ord, Some(3));
    }

    #[test]
    fn ord_zero_rejected() {
        assert!(parse(&["task", "ord:0"]).is_err());
    }

    #[test]
    fn ord_non_integer_rejected() {
        assert!(parse(&["task", "ord:abc"]).is_err());
    }

    #[test]
    fn ord_empty_value_rejected() {
        assert!(parse(&["task", "ord:"]).is_err());
    }

    #[test]
    fn duplicate_category_rejected() {
        assert!(parse(&["task", "c:a", "c:b"]).is_err());
    }

    #[test]
    fn duplicate_category_across_aliases_rejected() {
        // `p:` is just an alias of `c:` — using both forms still counts as duplicate.
        assert!(parse(&["task", "p:a", "c:b"]).is_err());
    }

    #[test]
    fn duplicate_ord_rejected() {
        assert!(parse(&["task", "ord:1", "ord:2"]).is_err());
    }

    #[test]
    fn duplicate_est_rejected() {
        assert!(parse(&["task", "est:1h", "est:30m"]).is_err());
    }

    #[test]
    fn empty_category_value_clear_error() {
        let err = parse(&["task", "c:"]).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("required"), "got: {msg}");
    }

    #[test]
    fn category_with_colon_is_rejected() {
        let err = parse(&["task", "c:a:b"]).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("invalid category"), "got: {msg}");
    }

    #[test]
    fn negative_est_rejected() {
        assert!(parse(&["task", "est:-1h"]).is_err());
    }

    #[test]
    fn no_text_returns_none_text() {
        let p = parse(&["c:a"]).unwrap();
        assert!(p.text.is_none());
        assert_eq!(p.category, Some(Category::A));
    }

    #[test]
    fn trailing_bare_duration_becomes_estimate() {
        let p = parse(&["Buy", "milk", "30m"]).unwrap();
        assert_eq!(p.text.as_deref(), Some("Buy milk"));
        assert_eq!(p.est_secs, Some(1800));
    }

    #[test]
    fn leading_bare_duration_becomes_estimate() {
        let p = parse(&["4.5h", "plan", "sprint"]).unwrap();
        assert_eq!(p.text.as_deref(), Some("plan sprint"));
        assert_eq!(p.est_secs, Some(4 * 3600 + 1800));
    }

    #[test]
    fn explicit_est_wins_over_bare_token() {
        let p = parse(&["Buy", "milk", "30m", "est:1h"]).unwrap();
        // est: was set explicitly, so the trailing 30m stays as text.
        assert_eq!(p.est_secs, Some(3600));
        assert_eq!(p.text.as_deref(), Some("Buy milk 30m"));
    }

    #[test]
    fn bare_number_without_unit_stays_text() {
        let p = parse(&["Read", "5"]).unwrap();
        assert_eq!(p.text.as_deref(), Some("Read 5"));
        assert_eq!(p.est_secs, None);
    }

    #[test]
    fn single_duration_token_is_text_not_estimate() {
        // Nothing left if we stripped it, so it stays text.
        let p = parse(&["30m"]).unwrap();
        assert_eq!(p.text.as_deref(), Some("30m"));
        assert_eq!(p.est_secs, None);
    }

    #[test]
    fn split_estimate_prefers_trailing() {
        let (text, est) = split_estimate("1h some task 30m");
        assert_eq!(text, "1h some task");
        assert_eq!(est, Some(1800));
    }
}
