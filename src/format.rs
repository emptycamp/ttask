use crate::model::{Priority, Status, Task};
use crate::store::revert::HistoryEntry;
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, Utc};
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

// Visual layout for list rows. All widths are character columns.
const ID_W: usize = 3;
const PRI_W: usize = 1;
const TEXT_W: usize = 38;
const DUE_W: usize = 8;
const EST_W: usize = 4;

const DIVIDER_WIDTH: usize = 4 + ID_W + 2 + PRI_W + 2 + TEXT_W + 2 + DUE_W + 2 + EST_W;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ListMode {
    /// Compact view: at most two day groups (today + next day with tasks), each
    /// capped at 3 rows.
    Compact,
    /// Show every task — no per-day limit, no day-group cap.
    Full,
}

/// Maximum day groups shown in compact mode.
pub const COMPACT_MAX_DAYS: usize = 2;
/// Maximum rows shown per day in compact mode.
pub const COMPACT_MAX_PER_DAY: usize = 3;

pub fn format_list(tasks: &[Task], opts: &RenderOptions, mode: ListMode) -> String {
    if opts.markdown {
        return format_list_md(tasks, mode);
    }
    if tasks.is_empty() {
        return "  No tasks.\n".to_string();
    }

    let today = Local::now().date_naive();
    let now_utc = Utc::now();

    // Sort by (effective day, priority, time-of-day) so each day's group is
    // priority-ordered. Active overdue rolls into today via effective_day.
    let mut tasks: Vec<&Task> = tasks.iter().collect();
    tasks.sort_by_key(|t| sort_key(t, today));

    let mut out = String::new();
    out.push_str(&header_row(opts));
    out.push('\n');
    out.push_str(&styled_divider(opts));
    out.push('\n');

    // Pre-group tasks by effective day so overdue active tasks land under "Today".
    let mut groups: Vec<(NaiveDate, Vec<&Task>)> = Vec::new();
    for t in tasks {
        let day = effective_day(t, today);
        if groups.last().map(|(d, _)| *d) != Some(day) {
            groups.push((day, Vec::new()));
        }
        groups.last_mut().unwrap().1.push(t);
    }

    // Compact view: cap day groups (today + next with tasks = 2) and rows per day (3).
    // No trailing "+N days hidden" footer — the compact view should stay quiet.
    let visible_groups: Vec<(NaiveDate, Vec<&Task>)> = match mode {
        ListMode::Compact => groups.into_iter().take(COMPACT_MAX_DAYS).collect(),
        ListMode::Full => groups,
    };

    let per_day_cap = match mode {
        ListMode::Compact => Some(COMPACT_MAX_PER_DAY),
        ListMode::Full => None,
    };

    for (i, (day, day_tasks)) in visible_groups.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let total = day_tasks.len();
        let show = per_day_cap.map(|cap| cap.min(total)).unwrap_or(total);
        let hidden_in_day = total - show;
        out.push_str(&day_header(*day, today, hidden_in_day, opts));
        out.push('\n');
        for t in day_tasks.iter().take(show) {
            out.push_str(&format_list_row(t, now_utc, opts));
            out.push('\n');
        }
    }
    out
}

fn header_row(opts: &RenderOptions) -> String {
    let row = format!(
        "    {:>w_id$}  {:>w_pri$}  {:<w_text$}  {:>w_due$}  {:>w_est$}",
        "ID",
        "P",
        "Description",
        "Due",
        "Est",
        w_id = ID_W,
        w_pri = PRI_W,
        w_text = TEXT_W,
        w_due = DUE_W,
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

fn day_header(day: NaiveDate, today: NaiveDate, hidden: usize, opts: &RenderOptions) -> String {
    let label = if day == today {
        "Today".to_string()
    } else if day == today + Duration::days(1) {
        "Tomorrow".to_string()
    } else if day == today - Duration::days(1) {
        "Yesterday".to_string()
    } else {
        let weekday = weekday_short(day.weekday());
        format!("{weekday}, {}", day.format("%b %-d"))
    };

    // Right-align "+N" overflow marker at the end of the divider width.
    let suffix = if hidden > 0 {
        format!("+{hidden}")
    } else {
        String::new()
    };
    let pad = DIVIDER_WIDTH.saturating_sub(2 + label.chars().count() + suffix.chars().count());

    if opts.color {
        let label_styled = format!("{}", label.clone().with(Color::Cyan));
        let suffix_styled = if suffix.is_empty() {
            String::new()
        } else {
            format!("{}", suffix.with(Color::DarkGrey))
        };
        format!("  {label_styled}{}{suffix_styled}", " ".repeat(pad))
    } else {
        format!("  {label}{}{suffix}", " ".repeat(pad))
    }
}

fn weekday_short(w: chrono::Weekday) -> &'static str {
    match w {
        chrono::Weekday::Mon => "Mon",
        chrono::Weekday::Tue => "Tue",
        chrono::Weekday::Wed => "Wed",
        chrono::Weekday::Thu => "Thu",
        chrono::Weekday::Fri => "Fri",
        chrono::Weekday::Sat => "Sat",
        chrono::Weekday::Sun => "Sun",
    }
}

pub fn format_list_row(task: &Task, now: DateTime<Utc>, opts: &RenderOptions) -> String {
    let due_str = format_relative(task.due, now);
    let est_str = format_est(task.est_secs);
    let text = truncate(&sanitize_for_terminal(&task.text), TEXT_W);

    let pri_letter = task.priority.to_string();
    let pri_styled = if opts.color {
        format!("{}", pri_letter.with(priority_color(task.priority)))
    } else {
        pri_letter
    };

    // H8: a leading two-character status badge so `--all` (which mixes active /
    // completed / deleted rows) is readable. Active rows show "  " so the existing
    // active-only view looks unchanged.
    let status_badge = status_badge(task.status, opts);

    // {pri_styled} is a 1-char visible cell; ANSI escape codes don't count toward
    // the format-string width, so the surrounding columns still line up.
    format!(
        "  {status_badge}{:>w_id$}  {pri_styled}  {:<w_text$}  {:>w_due$}  {:>w_est$}",
        task.id,
        text,
        due_str,
        est_str,
        w_id = ID_W,
        w_text = TEXT_W,
        w_due = DUE_W,
        w_est = EST_W,
    )
}

/// Two-character status badge prefix used in list rows. Active is blank so the
/// active-only view (the common case) shows no clutter; completed and deleted rows
/// get clearly visible markers that survive even when color is disabled.
fn status_badge(status: Status, opts: &RenderOptions) -> String {
    let marker = match status {
        Status::Active => "  ",
        Status::Completed => "✓ ",
        Status::SoftDeleted => "✗ ",
    };
    if !opts.color || status == Status::Active {
        return marker.to_string();
    }
    let color = match status {
        Status::Completed => Color::Green,
        Status::SoftDeleted => Color::DarkGrey,
        Status::Active => Color::Reset,
    };
    format!("{}", marker.with(color))
}

/// Strip ANSI escape sequences and unrenderable control bytes from user-supplied text
/// before showing it. The DB is shared / sync'd, so a malicious task description
/// could otherwise embed terminal escapes (cursor moves, color changes, alternate
/// screen) that would let a writer mess up the reader's terminal. Tabs collapse to a
/// single space because they otherwise break column alignment.
pub fn sanitize_for_terminal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            // Newlines and tabs break table alignment.
            '\n' | '\r' => out.push(' '),
            '\t' => out.push(' '),
            // ESC (0x1B) starts ANSI escape sequences — drop it. Same for the other
            // C0/C1 control bytes (except TAB/LF/CR which we handled above and the
            // ordinary printable range starting at 0x20).
            c if (c as u32) < 0x20 => out.push('·'),
            c if (c as u32) == 0x7f => out.push('·'),
            // C1 control range U+0080..=U+009F (some terminals interpret these).
            c if (0x80..=0x9F).contains(&(c as u32)) => out.push('·'),
            c => out.push(c),
        }
    }
    out
}

/// The day a task should be grouped under in the list. For active tasks whose due
/// date has already passed, this rolls forward to `today` — overdue work is something
/// to do *now*, not a piece of history. Completed and deleted tasks keep their actual
/// day so the `--completed` / `--deleted` filters stay chronological.
pub fn effective_day(t: &Task, today: NaiveDate) -> NaiveDate {
    let local: DateTime<Local> = t.due.into();
    let day = local.date_naive();
    if t.status == Status::Active && day < today {
        today
    } else {
        day
    }
}

/// Canonical task ordering used by both the CLI list and the interactive TUI:
/// by effective day, then priority within the day, then time-of-day.
pub fn sort_key(t: &Task, today: NaiveDate) -> (NaiveDate, u8, chrono::NaiveTime) {
    let local: DateTime<Local> = t.due.into();
    let pri_rank = match t.priority {
        Priority::A => 0u8,
        Priority::B => 1u8,
        Priority::C => 2u8,
    };
    (effective_day(t, today), pri_rank, local.time())
}

/// Friendly day label. Past dates are only seen on non-active tasks (completed,
/// deleted) so "Yesterday" / weekday-name are still useful — they're not used to
/// label active overdue tasks, which roll into "Today" via `effective_day`.
pub fn day_label(day: NaiveDate, today: NaiveDate) -> String {
    if day == today {
        "Today".to_string()
    } else if day == today + Duration::days(1) {
        "Tomorrow".to_string()
    } else {
        let weekday = match day.weekday() {
            chrono::Weekday::Mon => "Mon",
            chrono::Weekday::Tue => "Tue",
            chrono::Weekday::Wed => "Wed",
            chrono::Weekday::Thu => "Thu",
            chrono::Weekday::Fri => "Fri",
            chrono::Weekday::Sat => "Sat",
            chrono::Weekday::Sun => "Sun",
        };
        format!("{weekday}, {}", day.format("%b %-d"))
    }
}

pub fn format_info(task: &Task, opts: &RenderOptions) -> String {
    if opts.markdown {
        return format_info_md(task);
    }
    let now: DateTime<Utc> = Utc::now();
    let due_local: DateTime<Local> = task.due.into();
    let created_local: DateTime<Local> = task.created_at.into();
    let status = match task.status {
        Status::Active => "active",
        Status::Completed => "completed",
        Status::SoftDeleted => "deleted",
    };
    let due_relative = format_relative(task.due, now);

    let priority_str = if opts.color {
        format!(
            "{}",
            task.priority
                .to_string()
                .with(priority_color(task.priority))
        )
    } else {
        task.priority.to_string()
    };

    let mut out = format!(
        "Task #{}\n  Text:     {}\n  Priority: {}\n  Status:   {}\n  Due:      {} ({})\n  Est:      {}\n  Created:  {}\n",
        task.id,
        sanitize_for_terminal(&task.text),
        priority_str,
        status,
        due_relative,
        due_local.format("%Y-%m-%d %H:%M"),
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
        // Md mode is for agents — always render the verbose summary regardless of
        // the human-view `verbose` flag.
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
    sorted.sort_by(|a, b| b.0.cmp(&a.0));
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

/// Render tasks as a markdown document grouped by day. Targeted at LLM agents:
/// stable column order, explicit headings, no ANSI/Unicode decoration, and absolute
/// timestamps so the agent can reason about dates without needing "now". Unlike the
/// human view, the agent view spells out *what's hidden* and *how to ask for more* —
/// agents can't see inline color/spacing cues, so we have to say it in text.
pub fn format_list_md(tasks: &[Task], mode: ListMode) -> String {
    let mut out = String::new();
    if tasks.is_empty() {
        out.push_str("# Tasks\n\n_No tasks._\n");
        return out;
    }

    let today = Local::now().date_naive();
    let now_utc = Utc::now();

    let mut tasks: Vec<&Task> = tasks.iter().collect();
    tasks.sort_by_key(|t| sort_key(t, today));

    let mut groups: Vec<(NaiveDate, Vec<&Task>)> = Vec::new();
    for t in tasks {
        let day = effective_day(t, today);
        if groups.last().map(|(d, _)| *d) != Some(day) {
            groups.push((day, Vec::new()));
        }
        groups.last_mut().unwrap().1.push(t);
    }

    let total_days = groups.len();
    let total_tasks: usize = groups.iter().map(|(_, t)| t.len()).sum();

    let visible_groups: Vec<(NaiveDate, Vec<&Task>)> = match mode {
        ListMode::Compact => groups.into_iter().take(COMPACT_MAX_DAYS).collect(),
        ListMode::Full => groups,
    };
    let per_day_cap = match mode {
        ListMode::Compact => Some(COMPACT_MAX_PER_DAY),
        ListMode::Full => None,
    };

    let hidden_days = total_days - visible_groups.len();
    let mut shown_tasks = 0usize;
    let mut hidden_within_days = 0usize;
    for (_, day_tasks) in &visible_groups {
        let total = day_tasks.len();
        let show = per_day_cap.map(|cap| cap.min(total)).unwrap_or(total);
        shown_tasks += show;
        hidden_within_days += total - show;
    }
    let hidden_total = total_tasks - shown_tasks;

    // Heading carries the visible/total counts so an agent can see at a glance
    // whether anything was truncated, even before scanning the tables.
    let visible_days = visible_groups.len();
    if hidden_total > 0 {
        out.push_str(&format!(
            "# Tasks ({shown_tasks} shown / {total_tasks} total, {visible_days} of {total_days} day{} visible)\n\n",
            if total_days == 1 { "" } else { "s" },
        ));
    } else {
        out.push_str(&format!(
            "# Tasks ({total_tasks} task{}, {total_days} day{})\n\n",
            if total_tasks == 1 { "" } else { "s" },
            if total_days == 1 { "" } else { "s" },
        ));
    }

    for (day, day_tasks) in &visible_groups {
        let label = md_day_label(*day, today);
        let total = day_tasks.len();
        let show = per_day_cap.map(|cap| cap.min(total)).unwrap_or(total);
        let hidden_in_day = total - show;
        let header = if hidden_in_day > 0 {
            format!("## {label} (+{hidden_in_day} more)")
        } else {
            format!("## {label}")
        };
        out.push_str(&format!("{header}\n\n"));
        out.push_str("| ID | Pri | Status | Description | Due | Est |\n");
        out.push_str("|---:|:---:|:-------|:------------|:----|----:|\n");
        for t in day_tasks.iter().take(show) {
            let due_local: DateTime<Local> = t.due.into();
            let due_rel = format_relative(t.due, now_utc);
            let text = sanitize_for_md(&t.text);
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} ({}) | {} |\n",
                t.id,
                t.priority,
                status_label(t.status),
                text,
                due_rel,
                due_local.format("%Y-%m-%d %H:%M"),
                format_est(t.est_secs),
            ));
        }
        out.push('\n');
    }

    // Agent-oriented "what's hidden + how to reach it" footer. Only shown when the
    // compact view actually elided something, and always paired with the exact
    // command that reveals the rest.
    if hidden_days > 0 || hidden_within_days > 0 {
        let mut bits: Vec<String> = Vec::new();
        if hidden_within_days > 0 {
            bits.push(format!(
                "+{hidden_within_days} task{} hidden within shown days",
                if hidden_within_days == 1 { "" } else { "s" },
            ));
        }
        if hidden_days > 0 {
            bits.push(format!(
                "+{hidden_days} day{} hidden after the shown days",
                if hidden_days == 1 { "" } else { "s" },
            ));
        }
        out.push_str(&format!("_Truncated: {}._\n", bits.join(", ")));
        out.push_str(
            "_To see every active task: `task list --active --format md`. \
             To include completed and deleted: `task list --all --format md`. \
             To inspect one task: `task info <ID> --format md`._\n",
        );
    }

    out
}

fn md_day_label(day: NaiveDate, today: NaiveDate) -> String {
    let prefix = if day == today {
        "Today"
    } else if day == today + Duration::days(1) {
        "Tomorrow"
    } else if day == today - Duration::days(1) {
        "Yesterday"
    } else {
        ""
    };
    if prefix.is_empty() {
        format!("{} ({})", day.format("%A"), day.format("%Y-%m-%d"))
    } else {
        format!("{prefix} ({})", day.format("%Y-%m-%d"))
    }
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
    let now: DateTime<Utc> = Utc::now();
    let due_local: DateTime<Local> = task.due.into();
    let created_local: DateTime<Local> = task.created_at.into();
    let due_relative = format_relative(task.due, now);

    let mut out = format!(
        "# Task #{}\n\n- **Text:** {}\n- **Priority:** {}\n- **Status:** {}\n- **Due:** {} ({})\n- **Est:** {}\n- **Created:** {}\n",
        task.id,
        sanitize_for_md(&task.text),
        task.priority,
        status_label(task.status),
        due_relative,
        due_local.format("%Y-%m-%d %H:%M"),
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

/// Markdown history view, always rendered verbose. The human view treats `-v` as
/// opt-in to keep the default scan-friendly, but the md view is for LLM agents —
/// they want the full per-field diff every time, so we hand them
/// `summary_verbose()` regardless of the flag.
pub fn format_history_md(entries: &[(u64, HistoryEntry)]) -> String {
    if entries.is_empty() {
        return "# History\n\n_No history._\n".to_string();
    }
    let mut sorted: Vec<&(u64, HistoryEntry)> = entries.iter().collect();
    sorted.sort_by(|a, b| b.0.cmp(&a.0));
    let mut out = String::from("# History\n\n");
    out.push_str("| ID | When | Event |\n");
    out.push_str("|---:|:-----|:------|\n");
    for (id, entry) in sorted {
        let local: DateTime<Local> = entry.timestamp.into();
        out.push_str(&format!(
            "| {} | {} | {} |\n",
            id,
            local.format("%Y-%m-%d %H:%M"),
            sanitize_for_md(&entry.op.summary_verbose()),
        ));
    }
    out
}

fn priority_color(p: Priority) -> Color {
    match p {
        Priority::A => Color::Red,
        Priority::B => Color::Yellow,
        Priority::C => Color::DarkGrey,
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
///
/// Use this for things that have *already happened* (history events) — the natural
/// reading is "how long ago did this happen?", not "when is it due?".
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

/// Human-friendly relative time. Examples: "now", "in 5m", "in 4h", "in 3d".
///
/// Anything in the past collapses to "now" — an overdue task is something to do
/// *now*, not a piece of trivia about how long ago it lapsed.
///
/// Rounds to the nearest unit so that "2h minus a microsecond" reads as "in 2h"
/// rather than the floored "in 1h" you'd get from integer division.
pub fn format_relative(target: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let diff = target - now;
    let secs = diff.num_seconds();
    if secs < 45 {
        return "now".to_string();
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
    format!("in {unit}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Priority, Status};
    use chrono::TimeZone;

    fn make_task(id: u32, text: &str, priority: Priority, due: DateTime<Utc>) -> Task {
        Task {
            id,
            text: text.to_string(),
            priority,
            due,
            est_secs: 1800,
            status: Status::Active,
            created_at: Utc::now(),
            completed_at: None,
            deleted_at: None,
        }
    }

    fn base() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 18, 12, 0, 0).unwrap()
    }

    #[test]
    fn format_list_empty_returns_no_tasks_message() {
        let opts = RenderOptions::no_color();
        let output = format_list(&[], &opts, ListMode::Full);
        assert!(output.contains("No tasks"));
    }

    #[test]
    fn format_list_rolls_overdue_active_into_today() {
        // Active task with due date in the past should appear under "Today".
        let now = Utc::now();
        let past_task = make_task(1, "overdue task", Priority::A, now - Duration::days(2));
        let opts = RenderOptions::no_color();
        let out = format_list(&[past_task], &opts, ListMode::Full);
        assert!(out.contains("Today"), "expected Today header, got:\n{out}");
        assert!(
            !out.contains("Yesterday"),
            "should not show Yesterday for active overdue:\n{out}"
        );
        assert!(out.contains("overdue task"));
    }

    #[test]
    fn format_list_completed_keeps_original_day() {
        // Completed tasks (shown only via --completed) keep their actual day even if past.
        let now = Utc::now();
        let mut t = make_task(1, "done task", Priority::B, now - Duration::days(2));
        t.status = Status::Completed;
        t.completed_at = Some(now);
        let opts = RenderOptions::no_color();
        let out = format_list(&[t], &opts, ListMode::Full);
        // Should NOT collapse into Today
        assert!(
            !out.contains("Today"),
            "completed should not roll into today:\n{out}"
        );
    }

    #[test]
    fn format_list_includes_header_row() {
        let task = make_task(1, "Buy milk", Priority::B, base());
        let opts = RenderOptions::no_color();
        let output = format_list(&[task], &opts, ListMode::Full);
        assert!(output.contains("ID"));
        assert!(output.contains("Description"));
        assert!(output.contains("Due"));
        assert!(output.contains("Est"));
    }

    #[test]
    fn format_list_sorts_priority_a_before_b_within_a_day() {
        let t1 = make_task(1, "B task", Priority::B, base() + Duration::hours(1));
        let t2 = make_task(2, "A task", Priority::A, base() + Duration::hours(2));
        let opts = RenderOptions::no_color();
        let out = format_list(&[t1, t2], &opts, ListMode::Full);
        let pos_a = out.find("A task").unwrap();
        let pos_b = out.find("B task").unwrap();
        assert!(pos_a < pos_b, "expected A-priority before B-priority");
    }

    /// Build a UTC timestamp at noon local time, `day_offset` days from today. Anchoring
    /// on local-noon dodges DST edge cases and ambiguity around midnight rollover, so the
    /// test stays stable on any timezone/system clock.
    fn at_local_noon(day_offset: i64) -> DateTime<Utc> {
        use chrono::TimeZone;
        let today = Local::now().date_naive();
        let target = today + Duration::days(day_offset);
        let naive = target.and_hms_opt(12, 0, 0).unwrap();
        Local
            .from_local_datetime(&naive)
            .single()
            .expect("noon is never DST-ambiguous")
            .with_timezone(&Utc)
    }

    #[test]
    fn compact_caps_per_day_to_three_rows() {
        // Five tasks all on the same future day — compact must show only three and
        // surface the overflow count on the day header.
        let day = at_local_noon(2);
        let tasks: Vec<Task> = (0..5)
            .map(|i| {
                let mut t = make_task(
                    i + 1,
                    &format!("t{i}"),
                    Priority::B,
                    day + Duration::minutes(i as i64),
                );
                // Same priority + sequential minutes => stable sort order so t0..t2 are
                // guaranteed to be the "first three".
                t.created_at = day;
                t
            })
            .collect();
        let opts = RenderOptions::no_color();
        let out = format_list(&tasks, &opts, ListMode::Compact);
        for i in 0..3 {
            assert!(out.contains(&format!("t{i}")), "missing t{i} in:\n{out}");
        }
        for i in 3..5 {
            assert!(
                !out.contains(&format!("t{i}")),
                "t{i} should be hidden:\n{out}"
            );
        }
        assert!(
            out.contains("+2"),
            "expected +2 overflow marker on day header:\n{out}"
        );
    }

    #[test]
    fn compact_caps_apply_independently_to_each_visible_day() {
        // Two future days, each with five tasks — the per-day cap kicks in twice.
        let day1 = at_local_noon(1);
        let day2 = at_local_noon(2);
        let mut tasks: Vec<Task> = Vec::new();
        for i in 0..5 {
            tasks.push(make_task(
                i + 1,
                &format!("d1t{i}"),
                Priority::B,
                day1 + Duration::minutes(i as i64),
            ));
        }
        for i in 0..5 {
            tasks.push(make_task(
                i + 6,
                &format!("d2t{i}"),
                Priority::B,
                day2 + Duration::minutes(i as i64),
            ));
        }
        let opts = RenderOptions::no_color();
        let out = format_list(&tasks, &opts, ListMode::Compact);
        for i in 0..3 {
            assert!(
                out.contains(&format!("d1t{i}")),
                "missing d1t{i} in:\n{out}"
            );
            assert!(
                out.contains(&format!("d2t{i}")),
                "missing d2t{i} in:\n{out}"
            );
        }
        for i in 3..5 {
            assert!(
                !out.contains(&format!("d1t{i}")),
                "d1t{i} should be hidden:\n{out}"
            );
            assert!(
                !out.contains(&format!("d2t{i}")),
                "d2t{i} should be hidden:\n{out}"
            );
        }
        // Both day headers should carry "+2"; expect at least two occurrences.
        assert!(
            out.matches("+2").count() >= 2,
            "expected an overflow marker on each day header:\n{out}"
        );
    }

    #[test]
    fn compact_with_exactly_three_tasks_shows_no_overflow_marker() {
        let day = at_local_noon(1);
        let tasks: Vec<Task> = (0..3)
            .map(|i| {
                make_task(
                    i + 1,
                    &format!("t{i}"),
                    Priority::B,
                    day + Duration::minutes(i as i64),
                )
            })
            .collect();
        let opts = RenderOptions::no_color();
        let out = format_list(&tasks, &opts, ListMode::Compact);
        for i in 0..3 {
            assert!(out.contains(&format!("t{i}")), "missing t{i} in:\n{out}");
        }
        assert!(
            !out.contains('+'),
            "no overflow marker expected when at exactly the cap:\n{out}"
        );
    }

    #[test]
    fn full_mode_never_caps_per_day() {
        let day = at_local_noon(1);
        let tasks: Vec<Task> = (0..7)
            .map(|i| {
                make_task(
                    i + 1,
                    &format!("t{i}"),
                    Priority::B,
                    day + Duration::minutes(i as i64),
                )
            })
            .collect();
        let opts = RenderOptions::no_color();
        let out = format_list(&tasks, &opts, ListMode::Full);
        for i in 0..7 {
            assert!(out.contains(&format!("t{i}")), "missing t{i} in:\n{out}");
        }
        assert!(
            !out.contains('+'),
            "full mode must not emit an overflow marker:\n{out}"
        );
    }

    #[test]
    fn compact_limits_to_two_day_groups_and_adds_hidden_day_footer() {
        // Three distinct future days — compact should show two and announce the third.
        let tasks: Vec<Task> = (1..=3)
            .map(|i| {
                make_task(
                    i as u32,
                    &format!("t{}", i - 1),
                    Priority::B,
                    at_local_noon(i),
                )
            })
            .collect();
        let opts = RenderOptions::no_color();
        let out = format_list(&tasks, &opts, ListMode::Compact);
        assert!(out.contains("t0"), "expected first-day task in:\n{out}");
        assert!(out.contains("t1"), "expected second-day task in:\n{out}");
        assert!(!out.contains("t2"), "third day must be hidden in:\n{out}");
        assert!(
            !out.contains("more day"),
            "compact view must not emit a 'more days hidden' footer:\n{out}"
        );
    }

    #[test]
    fn compact_md_caps_per_day_to_three_rows_and_announces_what_is_hidden() {
        // The md view is for agents — when rows are truncated we must say so in text
        // (the per-day "(+N more)" chip plus a `_Truncated: ..._` footer) and also
        // tell the agent which command unhides them.
        let day = at_local_noon(2);
        let tasks: Vec<Task> = (0..5)
            .map(|i| {
                make_task(
                    i + 1,
                    &format!("t{i}"),
                    Priority::B,
                    day + Duration::minutes(i as i64),
                )
            })
            .collect();
        let out = format_list_md(&tasks, ListMode::Compact);
        for i in 0..3 {
            assert!(out.contains(&format!("t{i}")), "missing t{i} in:\n{out}");
        }
        for i in 3..5 {
            assert!(
                !out.contains(&format!("t{i}")),
                "t{i} should be hidden:\n{out}"
            );
        }
        assert!(
            out.contains("(+2 more)"),
            "expected (+2 more) on the markdown day heading in:\n{out}"
        );
        assert!(
            out.contains("+2 tasks hidden within shown days"),
            "expected per-day overflow disclosure in:\n{out}"
        );
        assert!(
            out.contains("`task list --active --format md`"),
            "expected an agent-facing command hint in:\n{out}"
        );
    }

    #[test]
    fn compact_md_hides_extra_days_and_calls_them_out_in_the_footer() {
        // Three distinct future days — compact md shows two and explicitly tells the
        // agent the third was hidden, plus how to get the rest.
        let tasks: Vec<Task> = (1..=3)
            .map(|i| {
                make_task(
                    i as u32,
                    &format!("t{}", i - 1),
                    Priority::B,
                    at_local_noon(i),
                )
            })
            .collect();
        let out = format_list_md(&tasks, ListMode::Compact);
        assert!(out.contains("t0"), "expected first-day task in:\n{out}");
        assert!(out.contains("t1"), "expected second-day task in:\n{out}");
        assert!(!out.contains("t2"), "third day must be hidden in:\n{out}");
        assert!(
            out.contains("+1 day hidden after the shown days"),
            "expected hidden-day disclosure in:\n{out}"
        );
        assert!(
            out.contains("`task list --active --format md`"),
            "expected agent-facing command hint in:\n{out}"
        );
        assert!(
            out.contains("`task list --all --format md`"),
            "expected agent-facing command hint for full status set:\n{out}"
        );
    }

    #[test]
    fn full_md_emits_no_truncation_footer() {
        // Full mode never truncates — no _Truncated_ footer should appear.
        let day = at_local_noon(1);
        let tasks: Vec<Task> = (0..5)
            .map(|i| {
                make_task(
                    i + 1,
                    &format!("t{i}"),
                    Priority::B,
                    day + Duration::minutes(i as i64),
                )
            })
            .collect();
        let out = format_list_md(&tasks, ListMode::Full);
        assert!(
            !out.contains("Truncated"),
            "Full mode must not emit a Truncated footer:\n{out}"
        );
        assert!(
            !out.contains("hidden"),
            "Full mode must not announce any hidden tasks:\n{out}"
        );
    }

    #[test]
    fn md_heading_includes_visible_and_total_when_truncated() {
        let day = at_local_noon(2);
        let tasks: Vec<Task> = (0..5)
            .map(|i| {
                make_task(
                    i + 1,
                    &format!("t{i}"),
                    Priority::B,
                    day + Duration::minutes(i as i64),
                )
            })
            .collect();
        let out = format_list_md(&tasks, ListMode::Compact);
        assert!(
            out.contains("3 shown / 5 total"),
            "heading should disclose visible / total counts:\n{out}"
        );
    }

    #[test]
    fn md_heading_omits_visible_total_when_nothing_hidden() {
        let day = at_local_noon(1);
        let tasks: Vec<Task> = (0..2)
            .map(|i| {
                make_task(
                    i + 1,
                    &format!("t{i}"),
                    Priority::B,
                    day + Duration::minutes(i as i64),
                )
            })
            .collect();
        let out = format_list_md(&tasks, ListMode::Compact);
        assert!(
            !out.contains("shown /"),
            "heading should be unambiguous when nothing is hidden:\n{out}"
        );
        assert!(out.contains("# Tasks"), "still has the top heading:\n{out}");
    }

    #[test]
    fn format_list_full_shows_all_tasks() {
        let tasks: Vec<Task> = (0..6)
            .map(|i| {
                make_task(
                    i + 1,
                    &format!("t{i}"),
                    Priority::B,
                    base() + Duration::minutes(i as i64),
                )
            })
            .collect();
        let opts = RenderOptions::no_color();
        let out = format_list(&tasks, &opts, ListMode::Full);
        for i in 0..6 {
            assert!(out.contains(&format!("t{i}")));
        }
        assert!(!out.contains("+"));
    }

    #[test]
    fn format_relative_now_for_small_diff() {
        let now = base();
        let s = format_relative(now + Duration::seconds(10), now);
        assert_eq!(s, "now");
    }

    #[test]
    fn format_relative_minutes_future() {
        let now = base();
        let s = format_relative(now + Duration::minutes(5), now);
        assert_eq!(s, "in 5m");
    }

    #[test]
    fn format_relative_hours_future() {
        let now = base();
        let s = format_relative(now + Duration::hours(4), now);
        assert_eq!(s, "in 4h");
    }

    #[test]
    fn format_relative_past_just_now() {
        let now = base();
        let s = format_relative_past(now - Duration::seconds(10), now);
        assert_eq!(s, "just now");
    }

    #[test]
    fn format_relative_past_minutes() {
        let now = base();
        let s = format_relative_past(now - Duration::minutes(10), now);
        assert_eq!(s, "10m ago");
    }

    #[test]
    fn format_relative_past_hours() {
        let now = base();
        let s = format_relative_past(now - Duration::hours(2), now);
        assert_eq!(s, "2h ago");
    }

    #[test]
    fn format_relative_past_collapses_to_now() {
        let now = base();
        let s = format_relative(now - Duration::days(3), now);
        assert_eq!(s, "now");
    }

    #[test]
    fn format_relative_overdue_minutes_is_now() {
        let now = base();
        let s = format_relative(now - Duration::minutes(30), now);
        assert_eq!(s, "now");
    }

    #[test]
    fn format_relative_months() {
        let now = base();
        let s = format_relative(now + Duration::days(35), now);
        assert_eq!(s, "in 1mo");
    }

    #[test]
    fn truncate_long_text_appends_ellipsis_char() {
        let long = "a".repeat(50);
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
    fn format_info_includes_relative_due() {
        let now = Utc::now();
        let task = make_task(1, "Buy milk", Priority::A, now + Duration::hours(2));
        let opts = RenderOptions::no_color();
        let out = format_info(&task, &opts);
        assert!(out.contains("Buy milk"));
        assert!(out.contains("in 2h"));
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
        assert!(!s.contains('\x1b'), "ESC should be removed, got: {s:?}");
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
        // H9 regression: tabs/newlines/ANSI in text broke alignment & opened the
        // door to terminal escape attacks.
        let opts = RenderOptions::no_color();
        let task = make_task(1, "evil\x1b[31m\ttext\n", Priority::B, base());
        let row = format_list_row(&task, base(), &opts);
        assert!(!row.contains('\x1b'));
        assert!(!row.contains('\t'));
    }

    #[test]
    fn format_list_row_shows_status_badge_for_deleted() {
        // H8 regression: --all used to render deleted rows identically to active.
        let opts = RenderOptions::no_color();
        let mut t = make_task(1, "gone", Priority::B, base());
        t.status = Status::SoftDeleted;
        let row = format_list_row(&t, base(), &opts);
        assert!(
            row.contains('✗'),
            "row should carry a deleted marker: {row}"
        );
    }

    #[test]
    fn format_list_row_shows_status_badge_for_completed() {
        let opts = RenderOptions::no_color();
        let mut t = make_task(1, "done", Priority::B, base());
        t.status = Status::Completed;
        let row = format_list_row(&t, base(), &opts);
        assert!(
            row.contains('✓'),
            "row should carry a completed marker: {row}"
        );
    }

    #[test]
    fn format_list_md_empty_renders_no_tasks_message() {
        let out = format_list_md(&[], ListMode::Full);
        assert!(out.starts_with("# Tasks"));
        assert!(out.contains("_No tasks._"));
    }

    #[test]
    fn format_list_md_emits_table_with_columns() {
        let now = Utc::now();
        let task = make_task(1, "Buy milk", Priority::A, now + Duration::hours(2));
        let out = format_list_md(&[task], ListMode::Full);
        assert!(out.contains("| ID | Pri | Status | Description | Due | Est |"));
        assert!(out.contains("|---:|:---:|:-------|:------------|:----|----:|"));
        assert!(out.contains("Buy milk"));
        assert!(out.contains("| A |"));
        assert!(out.contains("active"));
    }

    #[test]
    fn format_list_md_escapes_pipe_in_text() {
        let now = Utc::now();
        let task = make_task(1, "a | b", Priority::B, now + Duration::hours(1));
        let out = format_list_md(&[task], ListMode::Full);
        assert!(
            out.contains(r"a \| b"),
            "pipes in text must be escaped: {out}"
        );
    }

    #[test]
    fn format_list_md_compact_hides_extra_days_and_discloses_in_footer() {
        // Mirror of the compact_md test above; kept distinct because earlier tests
        // anchored on `Utc::now()` rather than `at_local_noon` so removing it would
        // lose coverage of that path. md mode is for agents, so the truncation must
        // be announced rather than silent.
        let now = Utc::now();
        let tasks: Vec<Task> = (0..3)
            .map(|i| {
                make_task(
                    i + 1,
                    &format!("t{i}"),
                    Priority::B,
                    now + Duration::days(i as i64 + 1),
                )
            })
            .collect();
        let out = format_list_md(&tasks, ListMode::Compact);
        assert!(out.contains("t0"));
        assert!(out.contains("t1"));
        assert!(!out.contains("t2"));
        assert!(
            out.contains("day hidden") || out.contains("days hidden"),
            "footer must announce the hidden day:\n{out}"
        );
        assert!(
            out.contains("`task list --active --format md`"),
            "footer must point at the unhiding command:\n{out}"
        );
    }

    #[test]
    fn format_info_md_emits_markdown_heading_and_bullets() {
        let now = Utc::now();
        let task = make_task(7, "Read book", Priority::A, now + Duration::hours(2));
        let out = format_info_md(&task);
        assert!(out.starts_with("# Task #7"));
        assert!(out.contains("- **Text:** Read book"));
        assert!(out.contains("- **Priority:** A"));
        assert!(out.contains("- **Status:** active"));
    }

    #[test]
    fn format_history_md_empty_returns_no_history_section() {
        let out = format_history_md(&[]);
        assert!(out.contains("# History"));
        assert!(out.contains("_No history._"));
    }

    #[test]
    fn format_history_md_emits_table() {
        let now = Utc::now();
        let task = make_task(1, "first", Priority::B, now);
        let entries = vec![(
            5,
            HistoryEntry {
                op: crate::store::revert::RevertOp::Added { task },
                timestamp: now,
            },
        )];
        let out = format_history_md(&entries);
        assert!(out.contains("| ID | When | Event |"));
        assert!(out.contains("| 5 |"));
        assert!(out.contains("added #1"));
    }

    #[test]
    fn format_history_md_always_renders_verbose_diff_for_edits() {
        // Md mode is always verbose — agents want the full diff every time.
        let now = Utc::now();
        let before = make_task(1, "before", Priority::B, now);
        let mut after = before.clone();
        after.text = "after".into();
        let entries = vec![(
            5,
            HistoryEntry {
                op: crate::store::revert::RevertOp::Edited { before, after },
                timestamp: now,
            },
        )];
        let out = format_history_md(&entries);
        assert!(
            out.contains("text \\\"before\\\"→\\\"after\\\"")
                || out.contains("text \"before\"→\"after\""),
            "md history must include the old→new text diff: {out}"
        );
    }

    #[test]
    fn sanitize_for_md_escapes_pipes_and_strips_controls() {
        let s = sanitize_for_md("a|b\tc\nd\x1be");
        assert_eq!(s, "a\\|b c d e");
    }
}
