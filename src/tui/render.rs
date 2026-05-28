use crate::format::format_est;
use crate::model::{Category, Task};
use crate::tui::events::PendingChange;
use crate::tui::App;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};
use std::collections::HashMap;

const PREFIX_W: usize = 2;
const ID_W: usize = 3;
const CAT_W: usize = 3;
const TEXT_W: usize = 40;
const ORD_W: usize = 4;
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
        " {:>w_p$}{:>w_id$}  {:>w_cat$}  {:<w_text$}  {:>w_ord$}  {:>w_est$}",
        "",
        "ID",
        "Cat",
        "Description",
        "Ord",
        "Est",
        w_p = PREFIX_W,
        w_id = ID_W,
        w_cat = CAT_W,
        w_text = TEXT_W,
        w_ord = ORD_W,
        w_est = EST_W,
    );
    let header = Paragraph::new(Span::styled(
        header_text,
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(header, chunks[0]);

    let visible = app.filtered_tasks();
    let mut items: Vec<ListItem> = Vec::new();
    let row_width = chunks[1].width.saturating_sub(2) as usize;

    for (i, task) in visible.iter().enumerate() {
        items.push(make_item(task, &app.pending, row_width, i == app.cursor));
    }

    let mut state = ListState::default();
    state.select(if items.is_empty() {
        None
    } else {
        Some(app.cursor.min(items.len() - 1))
    });

    let task_list = List::new(items)
        .block(Block::default().borders(Borders::ALL))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );

    ratatui::widgets::StatefulWidget::render(task_list, chunks[1], frame.buffer_mut(), &mut state);

    frame.render_widget(make_help_or_search(app), chunks[2]);
}

fn make_help_or_search<'a>(app: &'a App) -> Paragraph<'a> {
    if let Some(buf) = &app.search_input {
        return Paragraph::new(Line::from(vec![
            Span::raw(" /"),
            Span::raw(buf.as_str()),
            Span::styled("▏", Style::default().fg(Color::Cyan)),
            Span::raw("  "),
            Span::styled(
                "Enter apply · Esc cancel",
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }
    if !app.search_filter.is_empty() {
        return Paragraph::new(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                format!("filter: {}", app.search_filter),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(
                "  ·  / edit  ·  Esc clear filter",
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }
    Paragraph::new(Span::styled(
        " ↑↓ task · 1-9 reorder · ⏎/e edit · a add · c done · d del · / search · Esc quit ",
        Style::default().fg(Color::DarkGray),
    ))
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
    let pending_category = changes.iter().find_map(|c| {
        if let PendingChange::SetCategory(_, p) = c {
            Some(*p)
        } else {
            None
        }
    });

    let display_category = pending_category.unwrap_or(task.category);
    let category_char = display_category.to_string();
    let category_fg = match display_category {
        Category::A => Color::Red,
        Category::B => Color::Yellow,
        Category::C => Color::DarkGray,
    };

    let prefix = if has_complete {
        "✓ "
    } else if has_delete {
        "✗ "
    } else {
        "  "
    };

    let ord_str = task.ord.to_string();
    let est_str = format_est(task.est_secs);

    // Cat column is right-aligned to CAT_W; the styled letter is one char wide,
    // so pad with CAT_W-1 leading spaces before the styled span.
    let cat_pad = " ".repeat(CAT_W.saturating_sub(1));
    let left = format!(" {}{:>w_id$}  {cat_pad}", prefix, task.id, w_id = ID_W);
    let middle = format!(
        "  {:<w_text$}  {:>w_ord$}  {:>w_est$}",
        truncate(&task.text, TEXT_W),
        ord_str,
        est_str,
        w_text = TEXT_W,
        w_ord = ORD_W,
        w_est = EST_W,
    );

    let mut row_chars = left.chars().count() + 1 + middle.chars().count();
    let mut trailing = String::new();
    while row_chars < width {
        trailing.push(' ');
        row_chars += 1;
    }

    let mut category_style = Style::default().fg(category_fg);
    if has_complete {
        category_style = Style::default().fg(Color::Green);
    } else if has_delete {
        category_style = Style::default()
            .fg(Color::Red)
            .add_modifier(Modifier::CROSSED_OUT);
    } else if selected && display_category == Category::C {
        // The default C colour (DarkGray) clashes with the row-highlight bg
        // (also DarkGray), making the `C` invisible on the cursor row. Switch
        // to a high-contrast colour just for this case.
        category_style = Style::default().fg(Color::White);
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
        Span::styled(category_char, category_style.add_modifier(Modifier::BOLD)),
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
