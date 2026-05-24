use crate::error::Result;
use crate::format::{format_list, ListMode, RenderOptions};
use crate::model::{Status, Task};
use crate::store::Store;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Filter {
    Active,
    Completed,
    Deleted,
    All,
}

/// What the user explicitly asked for on the command line.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FilterChoice {
    pub filter: Filter,
    /// True if the user passed any of --active/--completed/--deleted/--all explicitly.
    pub explicit: bool,
}

pub fn resolve_filter(active: bool, completed: bool, deleted: bool, all: bool) -> FilterChoice {
    if all {
        FilterChoice {
            filter: Filter::All,
            explicit: true,
        }
    } else if completed {
        FilterChoice {
            filter: Filter::Completed,
            explicit: true,
        }
    } else if deleted {
        FilterChoice {
            filter: Filter::Deleted,
            explicit: true,
        }
    } else if active {
        FilterChoice {
            filter: Filter::Active,
            explicit: true,
        }
    } else {
        FilterChoice {
            filter: Filter::Active,
            explicit: false,
        }
    }
}

pub fn run(store: &Store, choice: FilterChoice, opts: &RenderOptions) -> Result<(String, u32)> {
    run_with_gc_count(store, choice, opts, 0)
}

pub fn run_with_gc_count(
    store: &Store,
    choice: FilterChoice,
    opts: &RenderOptions,
    gc_count: u32,
) -> Result<(String, u32)> {
    let tasks = store.all_tasks()?;

    let filtered: Vec<Task> = tasks
        .into_iter()
        .filter(|t| matches_filter(t.status, choice.filter))
        .collect();

    // Compact view only applies to the implicit default. Any explicit flag (including
    // --active) shows the full list.
    let mode = if choice.explicit {
        ListMode::Full
    } else {
        ListMode::Compact
    };
    let output = format_list(&filtered, opts, mode);
    Ok((output, gc_count))
}

fn matches_filter(status: Status, filter: Filter) -> bool {
    match filter {
        Filter::Active => status == Status::Active,
        Filter::Completed => status == Status::Completed,
        Filter::Deleted => status == Status::SoftDeleted,
        Filter::All => true,
    }
}

pub fn format_with_footer(output: &str, gc_count: u32) -> String {
    if gc_count > 0 {
        format!(
            "{output}  ({gc_count} task{} aged out)\n",
            if gc_count == 1 { "" } else { "s" }
        )
    } else {
        output.to_string()
    }
}

pub fn format_with_footer_md(output: &str, gc_count: u32) -> String {
    if gc_count > 0 {
        format!(
            "{output}\n_{gc_count} task{} aged out._\n",
            if gc_count == 1 { "" } else { "s" }
        )
    } else {
        output.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Priority, Status, Task};
    use chrono::Utc;
    use tempfile::tempdir;

    fn make_task(id: u32, status: Status) -> Task {
        Task {
            id,
            text: format!("task {id}"),
            priority: Priority::B,
            due: Utc::now(),
            est_secs: 1800,
            status,
            created_at: Utc::now(),
            completed_at: None,
            deleted_at: None,
        }
    }

    fn default_choice() -> FilterChoice {
        FilterChoice {
            filter: Filter::Active,
            explicit: false,
        }
    }

    #[test]
    fn list_active_only_by_default() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store.add_task(make_task(1, Status::Active)).unwrap();
        store.add_task(make_task(2, Status::Completed)).unwrap();

        let opts = RenderOptions::no_color();
        let (output, _) = run(&store, default_choice(), &opts).unwrap();
        assert!(output.contains("task 1"));
        assert!(!output.contains("task 2"));
    }

    #[test]
    fn list_completed_shows_only_completed() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store.add_task(make_task(1, Status::Active)).unwrap();
        store.add_task(make_task(2, Status::Completed)).unwrap();

        let opts = RenderOptions::no_color();
        let choice = FilterChoice {
            filter: Filter::Completed,
            explicit: true,
        };
        let (output, _) = run(&store, choice, &opts).unwrap();
        assert!(!output.contains("task 1"));
        assert!(output.contains("task 2"));
    }

    #[test]
    fn list_deleted_shows_only_deleted() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store.add_task(make_task(1, Status::Active)).unwrap();
        store.add_task(make_task(2, Status::SoftDeleted)).unwrap();

        let opts = RenderOptions::no_color();
        let choice = FilterChoice {
            filter: Filter::Deleted,
            explicit: true,
        };
        let (output, _) = run(&store, choice, &opts).unwrap();
        assert!(!output.contains("task 1"));
        assert!(output.contains("task 2"));
    }

    #[test]
    fn list_all_shows_every_status() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store.add_task(make_task(1, Status::Active)).unwrap();
        store.add_task(make_task(2, Status::Completed)).unwrap();
        store.add_task(make_task(3, Status::SoftDeleted)).unwrap();

        let opts = RenderOptions::no_color();
        let choice = FilterChoice {
            filter: Filter::All,
            explicit: true,
        };
        let (output, _) = run(&store, choice, &opts).unwrap();
        assert!(output.contains("task 1"));
        assert!(output.contains("task 2"));
        assert!(output.contains("task 3"));
    }

    /// Build a UTC due-time at noon local time, `day_offset` days from today. Noon
    /// dodges DST + midnight rollover so the test is stable on any system clock.
    fn due_at_local_noon(day_offset: i64) -> chrono::DateTime<Utc> {
        use chrono::{Local, TimeZone};
        let today = Local::now().date_naive();
        let naive = (today + chrono::Duration::days(day_offset))
            .and_hms_opt(12, 0, 0)
            .unwrap();
        Local
            .from_local_datetime(&naive)
            .single()
            .expect("noon is never DST-ambiguous")
            .with_timezone(&Utc)
    }

    #[test]
    fn list_default_compact_caps_to_two_day_groups() {
        // Three different days; default (implicit Active) should show two and silently
        // drop the third — no "+N more days hidden" footer in the compact view.
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        for i in 0..3 {
            let mut t = make_task(i + 1, Status::Active);
            t.due = due_at_local_noon(i as i64 + 1);
            store.add_task(t).unwrap();
        }

        let opts = RenderOptions::no_color();
        let (output, _) = run(&store, default_choice(), &opts).unwrap();
        assert!(
            !output.contains("more day"),
            "compact view must not emit a 'more days hidden' footer:\n{output}"
        );
    }

    #[test]
    fn list_default_compact_caps_to_three_per_day() {
        // Five tasks all on the same future day — default (implicit Active) caps at 3.
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let day = due_at_local_noon(1);
        for i in 0..5 {
            let mut t = make_task(i + 1, Status::Active);
            t.text = format!("row{i}");
            t.due = day + chrono::Duration::minutes(i as i64);
            store.add_task(t).unwrap();
        }

        let opts = RenderOptions::no_color();
        let (output, _) = run(&store, default_choice(), &opts).unwrap();
        for i in 0..3 {
            assert!(
                output.contains(&format!("row{i}")),
                "expected row{i} visible in:\n{output}"
            );
        }
        for i in 3..5 {
            assert!(
                !output.contains(&format!("row{i}")),
                "row{i} should be hidden in:\n{output}"
            );
        }
        assert!(
            output.contains("+2"),
            "expected +2 overflow marker in:\n{output}"
        );
    }

    #[test]
    fn list_explicit_active_does_not_truncate() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        for i in 0..3 {
            let mut t = make_task(i + 1, Status::Active);
            t.due = due_at_local_noon(i as i64 + 1);
            store.add_task(t).unwrap();
        }

        let opts = RenderOptions::no_color();
        let choice = FilterChoice {
            filter: Filter::Active,
            explicit: true,
        };
        let (output, _) = run(&store, choice, &opts).unwrap();
        assert!(
            !output.contains("more day"),
            "explicit --active should not hide days: {output}"
        );
    }

    #[test]
    fn list_explicit_active_does_not_cap_rows_per_day() {
        // Same data as the cap test, but with --active (explicit) — Full mode shows all.
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let day = due_at_local_noon(1);
        for i in 0..5 {
            let mut t = make_task(i + 1, Status::Active);
            t.text = format!("row{i}");
            t.due = day + chrono::Duration::minutes(i as i64);
            store.add_task(t).unwrap();
        }

        let opts = RenderOptions::no_color();
        let choice = FilterChoice {
            filter: Filter::Active,
            explicit: true,
        };
        let (output, _) = run(&store, choice, &opts).unwrap();
        for i in 0..5 {
            assert!(
                output.contains(&format!("row{i}")),
                "expected row{i} in explicit --active output:\n{output}"
            );
        }
        assert!(
            !output.contains('+'),
            "explicit --active must not show an overflow marker:\n{output}"
        );
    }

    #[test]
    fn gc_footer_appears_when_count_nonzero() {
        let out = format_with_footer("tasks\n", 3);
        assert!(out.contains("3 tasks aged out"));
    }

    #[test]
    fn gc_footer_absent_when_count_zero() {
        let out = format_with_footer("tasks\n", 0);
        assert!(!out.contains("aged out"));
    }

    #[test]
    fn resolve_filter_default_is_implicit_active() {
        let r = resolve_filter(false, false, false, false);
        assert_eq!(r.filter, Filter::Active);
        assert!(!r.explicit);
    }

    #[test]
    fn resolve_filter_active_flag_is_explicit() {
        let r = resolve_filter(true, false, false, false);
        assert_eq!(r.filter, Filter::Active);
        assert!(r.explicit);
    }

    #[test]
    fn resolve_filter_completed_flag() {
        let r = resolve_filter(false, true, false, false);
        assert_eq!(r.filter, Filter::Completed);
        assert!(r.explicit);
    }

    #[test]
    fn resolve_filter_deleted_flag() {
        let r = resolve_filter(false, false, true, false);
        assert_eq!(r.filter, Filter::Deleted);
        assert!(r.explicit);
    }

    #[test]
    fn resolve_filter_all_flag() {
        let r = resolve_filter(false, false, false, true);
        assert_eq!(r.filter, Filter::All);
        assert!(r.explicit);
    }
}
