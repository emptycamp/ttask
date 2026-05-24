use crate::format::{day_label, effective_day, format_est, format_relative};
use crate::model::{Priority, Task};
use crate::tui::events::PendingChange;
use crate::tui::App;
use chrono::{Local, Utc};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};
use std::collections::HashMap;

// Column widths must match between header and rows or the columns drift.
const PREFIX_W: usize = 2;
const ID_W: usize = 3;
const PRI_W: usize = 1;
const TEXT_W: usize = 36;
const DUE_W: usize = 8;
const EST_W: usize = 4;

pub fn draw(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header row
            Constraint::Min(3),    // list
            Constraint::Length(1), // help row
        ])
        .split(frame.area());

    let header_text = format!(
        " {:>w_p$}{:>w_id$}  {:>w_pri$}  {:<w_text$}  {:>w_due$}  {:>w_est$}",
        "",
        "ID",
        "P",
        "Description",
        "Due",
        "Est",
        w_p = PREFIX_W,
        w_id = ID_W,
        w_pri = PRI_W,
        w_text = TEXT_W,
        w_due = DUE_W,
        w_est = EST_W,
    );
    let header = Paragraph::new(Span::styled(
        header_text,
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(header, chunks[0]);

    // Build interleaved (day header, task) items and map app.cursor (task index) to the
    // raw list position so we highlight the right row.
    let today = Local::now().date_naive();
    let mut items: Vec<ListItem> = Vec::new();
    let mut task_to_raw: Vec<usize> = Vec::with_capacity(app.tasks.len());
    let mut current_day = None;
    let row_width = chunks[1].width.saturating_sub(2) as usize;

    for (i, task) in app.tasks.iter().enumerate() {
        let day = effective_day(task, today);
        if Some(day) != current_day {
            items.push(make_day_header(day, today, row_width));
            current_day = Some(day);
        }
        task_to_raw.push(items.len());
        items.push(make_item(task, &app.pending, row_width, i == app.cursor));
    }

    let mut state = ListState::default();
    state.select(task_to_raw.get(app.cursor).copied());

    // Setting only `bg` on the highlight style means each Span's foreground colour
    // (notably the priority letter) is preserved when the row is selected — Style::patch
    // copies fields from the highlight that are Some, leaving fg untouched.
    let task_list = List::new(items)
        .block(Block::default().borders(Borders::ALL))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );

    ratatui::widgets::StatefulWidget::render(task_list, chunks[1], frame.buffer_mut(), &mut state);

    let help = Paragraph::new(Span::styled(
        " ↑↓ task · ←→ day · a add · e edit · c complete · d delete · Shift+A/B/C priority · Esc quit ",
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(help, chunks[2]);
}

fn make_day_header(
    day: chrono::NaiveDate,
    today: chrono::NaiveDate,
    width: usize,
) -> ListItem<'static> {
    let label = day_label(day, today);
    let mut text = format!("  {label}");
    // Pad so the entire row is the same width — keeps the under-line look consistent.
    while text.chars().count() < width {
        text.push(' ');
    }
    ListItem::new(Line::from(Span::styled(
        text,
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )))
}

fn make_item(
    task: &Task,
    pending: &HashMap<u32, Vec<PendingChange>>,
    width: usize,
    selected: bool,
) -> ListItem<'static> {
    let changes = pending.get(&task.id).map(|v| v.as_slice()).unwrap_or(&[]);

    let has_complete = changes
        .iter()
        .any(|c| matches!(c, PendingChange::ToggleComplete(_)));
    let has_delete = changes
        .iter()
        .any(|c| matches!(c, PendingChange::ToggleDelete(_)));
    let pending_priority = changes.iter().find_map(|c| {
        if let PendingChange::SetPriority(_, p) = c {
            Some(*p)
        } else {
            None
        }
    });

    let display_priority = pending_priority.unwrap_or(task.priority);
    let priority_char = display_priority.to_string();
    let priority_fg = match display_priority {
        Priority::A => Color::Red,
        Priority::B => Color::Yellow,
        Priority::C => Color::DarkGray,
    };

    let prefix = if has_complete {
        "✓ "
    } else if has_delete {
        "✗ "
    } else {
        "  "
    };

    let now = Utc::now();
    let due_str = format_relative(task.due, now);
    let est_str = format_est(task.est_secs);

    // Build everything except the priority letter as plain text; the priority letter
    // is its own styled span so only it carries the colour.
    let left = format!(" {}{:>w_id$}  ", prefix, task.id, w_id = ID_W,);
    let middle = format!(
        "  {:<w_text$}  {:>w_due$}  {:>w_est$}",
        truncate(&task.text, TEXT_W),
        due_str,
        est_str,
        w_text = TEXT_W,
        w_due = DUE_W,
        w_est = EST_W,
    );

    let mut row_chars = left.chars().count() + 1 /* priority char */ + middle.chars().count();
    let mut trailing = String::new();
    while row_chars < width {
        trailing.push(' ');
        row_chars += 1;
    }

    // Build the row as one Line with the priority letter as its own styled Span. We
    // also tag the whole line with a row-level style so the selection highlight, when
    // applied, covers the full width — including trailing space.
    let mut priority_style = Style::default().fg(priority_fg);
    if has_complete {
        priority_style = Style::default().fg(Color::Green);
    } else if has_delete {
        priority_style = Style::default()
            .fg(Color::Red)
            .add_modifier(Modifier::CROSSED_OUT);
    }

    let line_style = if has_complete {
        Style::default().fg(Color::Green)
    } else if has_delete {
        Style::default()
            .fg(Color::Red)
            .add_modifier(Modifier::CROSSED_OUT)
    } else if selected {
        Style::default().fg(Color::White)
    } else {
        Style::default()
    };

    let line = Line::from(vec![
        Span::raw(left),
        Span::styled(priority_char, priority_style.add_modifier(Modifier::BOLD)),
        Span::raw(middle),
        Span::raw(trailing),
    ])
    .style(line_style);

    ListItem::new(line)
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
