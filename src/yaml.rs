use crate::error::{Error, Result};
use crate::model::{Category, Task};
use crate::time::parse_duration;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct TaskYaml {
    text: String,
    category: String,
    ord: u32,
    est: String,
}

pub fn to_yaml(task: &Task) -> Result<String> {
    let est = format_est(task.est_secs);
    let body = TaskYaml {
        text: task.text.clone(),
        category: task.category.to_string(),
        ord: task.ord,
        est,
    };
    let yaml = serde_yml::to_string(&body)?;
    let id = task.id;
    Ok(format!(
        "# Task #{id} — edit and save to apply. Comments are ignored.\n# category: A | B | C\n# est accepts: 10m, 1h, 30s, etc.\n{yaml}"
    ))
}

pub fn from_yaml(s: &str, task: &Task) -> Result<Task> {
    let stripped: String = s
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");

    let parsed: TaskYaml = serde_yml::from_str(&stripped)?;

    let category: Category = parsed
        .category
        .parse()
        .map_err(|e: String| Error::Parse(e))?;

    let est_secs = parse_duration(&parsed.est)
        .map(|d| d.num_seconds())
        .unwrap_or(task.est_secs);

    let mut updated = task.clone();
    updated.text = parsed.text;
    updated.category = category;
    updated.ord = parsed.ord;
    updated.est_secs = est_secs;
    Ok(updated)
}

fn format_est(secs: i64) -> String {
    if secs % 3600 == 0 {
        format!("{}h", secs / 3600)
    } else if secs % 60 == 0 {
        format!("{}m", secs / 60)
    } else {
        format!("{secs}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Category, Status};
    use chrono::Utc;

    fn make_task() -> Task {
        let now = Utc::now();
        Task {
            id: 42,
            text: "Buy milk".to_string(),
            category: Category::B,
            ord: 3,
            est_secs: 1800,
            status: Status::Active,
            created_at: now,
            updated_at: now,
            completed_at: None,
            deleted_at: None,
        }
    }

    #[test]
    fn roundtrip_preserves_fields() {
        let task = make_task();
        let yaml = to_yaml(&task).unwrap();
        let parsed = from_yaml(&yaml, &task).unwrap();
        assert_eq!(parsed.text, task.text);
        assert_eq!(parsed.category, task.category);
        assert_eq!(parsed.ord, task.ord);
        assert_eq!(parsed.est_secs, task.est_secs);
    }

    #[test]
    fn comments_are_stripped_before_parse() {
        let task = make_task();
        let yaml = to_yaml(&task).unwrap();
        assert!(from_yaml(&yaml, &task).is_ok());
    }

    #[test]
    fn malformed_yaml_returns_error() {
        let task = make_task();
        assert!(from_yaml("not: valid: yaml: :::", &task).is_err());
    }

    #[test]
    fn format_est_hours() {
        assert_eq!(format_est(7200), "2h");
    }

    #[test]
    fn format_est_minutes() {
        assert_eq!(format_est(1800), "30m");
    }

    #[test]
    fn format_est_seconds() {
        assert_eq!(format_est(90), "90s");
    }
}
