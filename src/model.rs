use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub type TaskId = u32;

/// Task category. Used both to bucket tasks visually and to drive the
/// auto-deletion clock — see `src/store/gc.rs` for the per-category cutoffs.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Category {
    A,
    #[default]
    B,
    C,
}

impl std::fmt::Display for Category {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::A => write!(f, "A"),
            Self::B => write!(f, "B"),
            Self::C => write!(f, "C"),
        }
    }
}

impl std::str::FromStr for Category {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "A" => Ok(Self::A),
            "B" => Ok(Self::B),
            "C" => Ok(Self::C),
            _ => Err(format!("invalid category '{s}', expected A, B, or C")),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Active,
    Completed,
    SoftDeleted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub text: String,
    pub category: Category,
    /// Manual display order. Lower values sort first. Reassigned when the user
    /// types a digit in the TUI or passes `ord:N` from the CLI.
    pub ord: u32,
    pub est_secs: i64,
    pub status: Status,
    pub created_at: DateTime<Utc>,
    /// Wall-clock of the most recent user-driven update to this task (edit, ord
    /// change, category change). The GC sweep uses this to decide whether an
    /// active task is stale enough to evict — touching a task resets its clock.
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_default_is_b() {
        assert_eq!(Category::default(), Category::B);
    }

    #[test]
    fn category_from_str_a() {
        assert_eq!("a".parse::<Category>().unwrap(), Category::A);
    }

    #[test]
    fn category_from_str_b() {
        assert_eq!("B".parse::<Category>().unwrap(), Category::B);
    }

    #[test]
    fn category_from_str_c() {
        assert_eq!("C".parse::<Category>().unwrap(), Category::C);
    }

    #[test]
    fn category_from_str_invalid() {
        assert!("X".parse::<Category>().is_err());
    }

    #[test]
    fn category_display() {
        assert_eq!(Category::A.to_string(), "A");
        assert_eq!(Category::B.to_string(), "B");
        assert_eq!(Category::C.to_string(), "C");
    }

    #[test]
    fn task_serde_bincode_roundtrip() {
        use chrono::Timelike;
        let now = Utc::now().with_nanosecond(0).unwrap_or(Utc::now());
        let task = Task {
            id: 1,
            text: "hello".to_string(),
            category: Category::A,
            ord: 1,
            est_secs: 600,
            status: Status::Active,
            created_at: now,
            updated_at: now,
            completed_at: None,
            deleted_at: None,
        };
        let encoded = bincode::serialize(&task).unwrap();
        let decoded: Task = bincode::deserialize(&encoded).unwrap();
        assert_eq!(task, decoded);
    }
}
