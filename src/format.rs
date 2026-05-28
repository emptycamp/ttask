use crate::model::{Category, Status, Task};
use crate::store::revert::HistoryEntry;
use chrono::{DateTime, Local, Utc};
use crossterm::style::{Color, Stylize};
use std::io::IsTerminal;

pub struct RenderOptions {
    pub color: bool,
    pub markdown: bool,
}

impl RenderOptions {
    pub fn detect() -> Self {
        let color = std::io::stdout().is_terminal() && std::env::var("NO_COLOR").is_err();
        Self {
            color,
            markdown: false,
        }
    }

    pub fn no_color() -> Self {
        Self {
            color: false,
            markdown: false,
        }
    }

    pub fn markdown() -> Self {
        Self {
            color: false,
            markdown: true,
        }
    }
}

// Visual layout for list rows.
const ID_W: usize = 3;
const CAT_W: usize = 3;
const TEXT_W: usize = 42;
const ORD_W: usize = 4;
const EST_W: usize = 4;

const DIVIDER_WIDTH: usize = 4 + ID_W + 2 + CAT_W + 2 + TEXT_W + 2 + ORD_W + 2 + EST_W;

pub fn format_list(tasks: &[Task], opts: &RenderOptions) -> String {
    if opts.markdown {
        return format_list_md(tasks);
    }
    if tasks.is_empty() {
        return "  No tasks.\n".to_string();
    }

    let mut tasks: Vec<&Task> = tasks.iter().collect();
    tasks.sort_by_key(|t| sort_key(t));

    let mut out = String::new();
    out.push_str(&header_row(opts));
    out.push('\n');
    out.push_str(&styled_divider(opts));
    out.push('\n');

    for t in tasks {
        out.push_str(&format_list_row(t, opts));
        out.push('\n');
    }
    out
}

fn header_row(opts: &RenderOptions) -> String {
    let row = format!(
        "    {:>w_id$}  {:>w_cat$}  {:<w_text$}  {:>w_ord$}  {:>w_est$}",
        "ID",
        "Cat",
        "Description",
        "Ord",
        "Est",
        w_id = ID_W,
        w_cat = CAT_W,
        w_text = TEXT_W,
        w_ord = ORD_W,
        w_est = EST_W,
    );
    if opts.color {
        format!("{}", row.with(Color::DarkGrey))
    } else {
        row
    }
}

fn styled_divider(opts: &RenderOptions) -> String {
    let line = "─".repeat(DIVIDER_WIDTH);
    if opts.color {
        format!("{}", line.with(Color::DarkGrey))
    } else {
        line
    }
}

pub fn format_list_row(task: &Task, opts: &RenderOptions) -> String {
    let est_str = format_est(task.est_secs);
    let ord_str = task.ord.to_string();
    let text = truncate(&sanitize_for_terminal(&task.text), TEXT_W);

    let cat_letter = task.category.to_string();
    let cat_styled = if opts.color {
        format!("{}", cat_letter.with(category_color(task.category)))
    } else {
        cat_letter
    };

    let status_badge = status_badge(task.status, opts);

    // The category cell is right-aligned to CAT_W, but the styled string contains
    // ANSI escapes that throw off `:>w_cat$` width math; pad manually.
    let cat_pad = " ".repeat(CAT_W.saturating_sub(1));
    format!(
        "  {status_badge}{:>w_id$}  {cat_pad}{cat_styled}  {:<w_text$}  {:>w_ord$}  {:>w_est$}",
        task.id,
        text,
        ord_str,
        est_str,
        w_id = ID_W,
        w_text = TEXT_W,
        w_ord = ORD_W,
        w_est = EST_W,
    )
}

fn status_badge(status: Status, opts: &RenderOptions) -> String {
    let (marker, color) = match status {
        Status::Active => ("  ", Color::Reset),
        Status::Completed => ("✓ ", Color::Green),
        Status::SoftDeleted => ("✗ ", Color::DarkGrey),
    };
    if !opts.color || status == Status::Active {
        return marker.to_string();
    }
    format!("{}", marker.with(color))
}

/// Strip ANSI escape sequences and unrenderable control bytes from user-supplied text
/// before showing it. The DB is shared / sync'd, so a malicious task description
/// could otherwise embed terminal escapes that would let a writer mess up the
/// reader's terminal.
pub fn sanitize_for_terminal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\n' | '\r' => out.push(' '),
            '\t' => out.push(' '),
            c if (c as u32) < 0x20 => out.push('·'),
            c if (c as u32) == 0x7f => out.push('·'),
            c if (0x80..=0x9F).contains(&(c as u32)) => out.push('·'),
            c => out.push(c),
        }
    }
    out
}

/// Canonical task ordering: by manual ord ascending, then by id as a tiebreaker.
pub fn sort_key(t: &Task) -> (u32, u32) {
    (t.ord, t.id)
}

pub fn format_info(task: &Task, opts: &RenderOptions) -> String {
    if opts.markdown {
        return format_info_md(task);
    }
    let created_local: DateTime<Local> = task.created_at.into();
    let status = match task.status {
        Status::Active => "active",
        Status::Completed => "completed",
        Status::SoftDeleted => "deleted",
    };

    let category_str = if opts.color {
        format!(
            "{}",
            task.category
                .to_string()
                .with(category_color(task.category))
        )
    } else {
        task.category.to_string()
    };

    let mut out = format!(
        "Task #{}\n  Text:     {}\n  Category: {}\n  Status:   {}\n  Ord:      {}\n  Est:      {}\n  Created:  {}\n",
        task.id,
        sanitize_for_terminal(&task.text),
        category_str,
        status,
        task.ord,
        format_est(task.est_secs),
        created_local.format("%Y-%m-%d %H:%M"),
    );

    if let Some(t) = task.completed_at {
        let local: DateTime<Local> = t.into();
        out.push_str(&format!("  Completed:{}\n", local.format("%Y-%m-%d %H:%M")));
    }
    if let Some(t) = task.deleted_at {
        let local: DateTime<Local> = t.into();
        out.push_str(&format!("  Deleted:  {}\n", local.format("%Y-%m-%d %H:%M")));
    }

    out
}

pub fn format_history(
    entries: &[(u64, HistoryEntry)],
    opts: &RenderOptions,
    verbose: bool,
) -> String {
    if opts.markdown {
        return format_history_md(entries);
    }
    if entries.is_empty() {
        return "  No history.\n".to_string();
    }
    let now = Utc::now();
    let mut out = String::new();
    out.push_str("    ID  When         Event\n");
    out.push_str(&"─".repeat(DIVIDER_WIDTH));
    out.push('\n');
    let mut sorted: Vec<&(u64, HistoryEntry)> = entries.iter().collect();
    sorted.sort_by_key(|(id, _)| std::cmp::Reverse(*id));
    for (id, entry) in sorted {
        let when = format_relative_past(entry.timestamp, now);
        let summary = if verbose {
            entry.op.summary_verbose()
        } else {
            entry.op.summary()
        };
        out.push_str(&format!("  {:>4}  {:<11}  {}\n", id, when, summary));
    }
    out
}

fn status_label(status: Status) -> &'static str {
    match status {
        Status::Active => "active",
        Status::Completed => "completed",
        Status::SoftDeleted => "deleted",
    }
}

/// Render tasks as a markdown document. Targeted at LLM agents: stable column
/// order, explicit headings, no ANSI/Unicode decoration.
pub fn format_list_md(tasks: &[Task]) -> String {
    let mut out = String::new();
    if tasks.is_empty() {
        out.push_str("# Tasks\n\n_No tasks._\n");
        return out;
    }

    let mut tasks: Vec<&Task> = tasks.iter().collect();
    tasks.sort_by_key(|t| sort_key(t));
    let total = tasks.len();

    out.push_str(&format!(
        "# Tasks ({total} task{})\n\n",
        if total == 1 { "" } else { "s" },
    ));

    out.push_str("| ID | Cat | Status | Ord | Description | Est |\n");
    out.push_str("|---:|:---:|:-------|---:|:------------|----:|\n");
    for t in &tasks {
        let text = sanitize_for_md(&t.text);
        let status = status_label(t.status);
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            t.id,
            t.category,
            status,
            t.ord,
            text,
            format_est(t.est_secs),
        ));
    }
    out.push('\n');

    out
}

/// Markdown is mostly plain text, but pipes and backticks would corrupt our table
/// cells. Newlines/tabs still collapse so each task is one row.
fn sanitize_for_md(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '|' => out.push_str("\\|"),
            '\n' | '\r' | '\t' => out.push(' '),
            c if (c as u32) < 0x20 => out.push(' '),
            c if (c as u32) == 0x7f => out.push(' '),
            c if (0x80..=0x9F).contains(&(c as u32)) => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

pub fn format_info_md(task: &Task) -> String {
    let created_local: DateTime<Local> = task.created_at.into();

    let mut out = format!(
        "# Task #{}\n\n- **Text:** {}\n- **Category:** {}\n- **Status:** {}\n- **Ord:** {}\n- **Est:** {}\n- **Created:** {}\n",
        task.id,
        sanitize_for_md(&task.text),
        task.category,
        status_label(task.status),
        task.ord,
        format_est(task.est_secs),
        created_local.format("%Y-%m-%d %H:%M"),
    );
    if let Some(t) = task.completed_at {
        let local: DateTime<Local> = t.into();
        out.push_str(&format!(
            "- **Completed:** {}\n",
            local.format("%Y-%m-%d %H:%M")
        ));
    }
    if let Some(t) = task.deleted_at {
        let local: DateTime<Local> = t.into();
        out.push_str(&format!(
            "- **Deleted:** {}\n",
            local.format("%Y-%m-%d %H:%M")
        ));
    }
    out
}

pub fn format_history_md(entries: &[(u64, HistoryEntry)]) -> String {
    if entries.is_empty() {
        return "# History\n\n_No history._\n".to_string();
    }
    let mut sorted: Vec<&(u64, HistoryEntry)> = entries.iter().collect();
    sorted.sort_by_key(|(id, _)| std::cmp::Reverse(*id));
    let now = Utc::now();
    let mut out = String::from("# History\n\n");
    out.push_str("| ID | When | Event |\n");
    out.push_str("|---:|:-----|:------|\n");
    for (id, entry) in sorted {
        out.push_str(&format!(
            "| {} | {} | {} |\n",
            id,
            format_relative_past(entry.timestamp, now),
            sanitize_for_md(&entry.op.summary_verbose()),
        ));
    }
    out
}

fn category_color(p: Category) -> Color {
    match p {
        Category::A => Color::Red,
        Category::B => Color::Yellow,
        Category::C => Color::DarkGrey,
    }
}

fn truncate(s: &str, width: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= width {
        s.to_string()
    } else {
        format!(
            "{}…",
            &chars[..width.saturating_sub(1)].iter().collect::<String>()
        )
    }
}

pub fn format_est(secs: i64) -> String {
    if secs <= 0 {
        return "0m".to_string();
    }
    if secs % 86400 == 0 {
        format!("{}d", secs / 86400)
    } else if secs % 3600 == 0 {
        format!("{}h", secs / 3600)
    } else if secs % 60 == 0 {
        format!("{}m", secs / 60)
    } else {
        format!("{secs}s")
    }
}

fn nearest(value: i64, unit: i64) -> i64 {
    (value + unit / 2) / unit
}

/// Relative time for past events: "5m ago", "2h ago", "just now".
pub fn format_relative_past(target: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let diff = now - target;
    let secs = diff.num_seconds();
    if secs < 45 {
        return "just now".to_string();
    }
    let unit = if secs < 3600 {
        format!("{}m", nearest(secs, 60).max(1))
    } else if secs < 86_400 {
        format!("{}h", nearest(secs, 3600).max(1))
    } else if secs < 30 * 86_400 {
        format!("{}d", nearest(secs, 86_400).max(1))
    } else if secs < 365 * 86_400 {
        format!("{}mo", nearest(secs, 30 * 86_400).max(1))
    } else {
        format!("{}y", nearest(secs, 365 * 86_400).max(1))
    };
    format!("{unit} ago")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Category, Status};
    use chrono::Duration;

    fn make_task(id: u32, text: &str, category: Category, ord: u32) -> Task {
        let now = Utc::now();
        Task {
            id,
            text: text.to_string(),
            category,
            ord,
            est_secs: 1800,
            status: Status::Active,
            created_at: now,
            updated_at: now,
            completed_at: None,
            deleted_at: None,
        }
    }

    #[test]
    fn format_list_empty_returns_no_tasks_message() {
        let opts = RenderOptions::no_color();
        let output = format_list(&[], &opts);
        assert!(output.contains("No tasks"));
    }

    #[test]
    fn format_list_sorts_by_ord_ascending() {
        let t1 = make_task(1, "second", Category::B, 5);
        let t2 = make_task(2, "first", Category::B, 1);
        let opts = RenderOptions::no_color();
        let out = format_list(&[t1, t2], &opts);
        let pos_first = out.find("first").unwrap();
        let pos_second = out.find("second").unwrap();
        assert!(pos_first < pos_second);
    }

    #[test]
    fn format_list_includes_ord_column_header() {
        let task = make_task(1, "Buy milk", Category::B, 1);
        let opts = RenderOptions::no_color();
        let output = format_list(&[task], &opts);
        assert!(output.contains("ID"));
        assert!(output.contains("Description"));
        assert!(output.contains("Ord"));
        assert!(output.contains("Est"));
        assert!(
            !output.contains("Due"),
            "list output must not mention Due:\n{output}"
        );
    }

    #[test]
    fn format_list_does_not_show_day_headings() {
        let t = make_task(1, "today task", Category::A, 1);
        let opts = RenderOptions::no_color();
        let out = format_list(&[t], &opts);
        assert!(
            !out.contains("Today") && !out.contains("Tomorrow") && !out.contains("Yesterday"),
            "list view must not group by day:\n{out}"
        );
    }

    #[test]
    fn format_list_does_not_emit_overflow_marker() {
        let tasks: Vec<Task> = (1..=10)
            .map(|i| make_task(i, &format!("t{i}"), Category::B, i))
            .collect();
        let opts = RenderOptions::no_color();
        let out = format_list(&tasks, &opts);
        assert!(
            !out.contains('+'),
            "list view must never emit a +N hidden indicator:\n{out}"
        );
    }

    #[test]
    fn format_relative_past_just_now() {
        let now = Utc::now();
        let s = format_relative_past(now - Duration::seconds(10), now);
        assert_eq!(s, "just now");
    }

    #[test]
    fn format_relative_past_minutes() {
        let now = Utc::now();
        let s = format_relative_past(now - Duration::minutes(10), now);
        assert_eq!(s, "10m ago");
    }

    #[test]
    fn format_relative_past_hours() {
        let now = Utc::now();
        let s = format_relative_past(now - Duration::hours(2), now);
        assert_eq!(s, "2h ago");
    }

    #[test]
    fn format_relative_past_days() {
        let now = Utc::now();
        let s = format_relative_past(now - Duration::days(3), now);
        assert_eq!(s, "3d ago");
    }

    #[test]
    fn truncate_long_text_appends_ellipsis_char() {
        let long = "a".repeat(60);
        let result = truncate(&long, 40);
        assert!(result.ends_with('…'));
        assert_eq!(result.chars().count(), 40);
    }

    #[test]
    fn format_est_minutes() {
        assert_eq!(format_est(1800), "30m");
    }

    #[test]
    fn format_est_hours() {
        assert_eq!(format_est(7200), "2h");
    }

    #[test]
    fn format_est_days() {
        assert_eq!(format_est(86400), "1d");
    }

    #[test]
    fn format_info_includes_ord() {
        let task = make_task(1, "Buy milk", Category::A, 7);
        let opts = RenderOptions::no_color();
        let out = format_info(&task, &opts);
        assert!(out.contains("Buy milk"));
        assert!(out.contains("Ord:"));
        assert!(out.contains('7'));
        assert!(!out.contains("Due:"), "info must not mention Due:\n{out}");
    }

    #[test]
    fn format_history_empty_returns_no_history() {
        let opts = RenderOptions::no_color();
        let out = format_history(&[], &opts, false);
        assert!(out.contains("No history"));
    }

    #[test]
    fn sanitize_strips_ansi_escape() {
        let s = sanitize_for_terminal("hi\x1b[31mred\x1b[0m bye");
        assert!(!s.contains('\x1b'));
    }

    #[test]
    fn sanitize_collapses_tabs_to_space() {
        let s = sanitize_for_terminal("a\tb\tc");
        assert_eq!(s, "a b c");
    }

    #[test]
    fn sanitize_replaces_newlines_with_space() {
        let s = sanitize_for_terminal("line1\nline2");
        assert!(!s.contains('\n'));
    }

    #[test]
    fn format_list_row_strips_control_chars_from_text() {
        let opts = RenderOptions::no_color();
        let task = make_task(1, "evil\x1b[31m\ttext\n", Category::B, 1);
        let row = format_list_row(&task, &opts);
        assert!(!row.contains('\x1b'));
        assert!(!row.contains('\t'));
    }

    #[test]
    fn format_list_row_shows_status_badge_for_deleted() {
        let opts = RenderOptions::no_color();
        let mut t = make_task(1, "gone", Category::B, 1);
        t.status = Status::SoftDeleted;
        let row = format_list_row(&t, &opts);
        assert!(row.contains('✗'));
    }

    #[test]
    fn format_list_row_shows_status_badge_for_completed() {
        let opts = RenderOptions::no_color();
        let mut t = make_task(1, "done", Category::B, 1);
        t.status = Status::Completed;
        let row = format_list_row(&t, &opts);
        assert!(row.contains('✓'));
    }

    #[test]
    fn format_list_md_empty_renders_no_tasks_message() {
        let out = format_list_md(&[]);
        assert!(out.starts_with("# Tasks"));
        assert!(out.contains("_No tasks._"));
    }

    #[test]
    fn format_list_md_emits_table_with_columns() {
        let task = make_task(1, "Buy milk", Category::A, 1);
        let out = format_list_md(&[task]);
        assert!(out.contains("| ID | Cat | Status | Ord | Description | Est |"));
        assert!(out.contains("Buy milk"));
        assert!(out.contains("| A |"));
        assert!(out.contains("active"));
        assert!(
            !out.contains("Due"),
            "md table must not mention Due:\n{out}"
        );
    }

    #[test]
    fn format_list_md_escapes_pipe_in_text() {
        let task = make_task(1, "a | b", Category::B, 1);
        let out = format_list_md(&[task]);
        assert!(out.contains(r"a \| b"));
    }

    #[test]
    fn format_info_md_emits_markdown_heading_and_bullets() {
        let task = make_task(7, "Read book", Category::A, 2);
        let out = format_info_md(&task);
        assert!(out.starts_with("# Task #7"));
        assert!(out.contains("- **Text:** Read book"));
        assert!(out.contains("- **Category:** A"));
        assert!(out.contains("- **Ord:** 2"));
        assert!(!out.contains("**Due**"));
    }

    #[test]
    fn format_history_md_empty_returns_no_history_section() {
        let out = format_history_md(&[]);
        assert!(out.contains("# History"));
        assert!(out.contains("_No history._"));
    }

    #[test]
    fn format_history_md_emits_table_with_relative_when() {
        let now = Utc::now();
        let task = make_task(1, "first", Category::B, 1);
        let entries = vec![(
            5,
            HistoryEntry {
                op: crate::store::revert::RevertOp::Added { task },
                timestamp: now - Duration::hours(2),
            },
        )];
        let out = format_history_md(&entries);
        assert!(out.contains("| ID | When | Event |"));
        assert!(out.contains("| 5 |"));
        assert!(out.contains("added #1"));
        assert!(out.contains("ago"), "When column should be relative: {out}");
    }

    #[test]
    fn sanitize_for_md_escapes_pipes_and_strips_controls() {
        let s = sanitize_for_md("a|b\tc\nd\x1be");
        assert_eq!(s, "a\\|b c d e");
    }
}
