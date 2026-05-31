use crate::format::{estimate_summary, format_est};
use crate::model::{Category, Task};
use crate::tui::App;
use chrono::Local;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

const PREFIX_W: usize = 2;
const ID_W: usize = 3;
const CAT_W: usize = 3;
const TEXT_W: usize = 44;
const EST_W: usize = 5;

pub fn draw(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header row
            Constraint::Min(3),    // list
            Constraint::Length(1), // estimate summary
            Constraint::Length(1), // help / search / status row
        ])
        .split(frame.area());

    let header_text = format!(
        " {:>w_p$}{:>w_id$}  {:>w_cat$}  {:<w_text$}  {:>w_est$}",
        "",
        "ID",
        "Cat",
        "Description",
        "Est",
        w_p = PREFIX_W,
        w_id = ID_W,
        w_cat = CAT_W,
        w_text = TEXT_W,
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
    let row_width = chunks[1].width.saturating_sub(2) as usize;
    let items: Vec<ListItem> = visible
        .iter()
        .enumerate()
        .map(|(i, task)| make_item(task, row_width, i == app.cursor))
        .collect();

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

    frame.render_widget(make_summary(app), chunks[2]);
    frame.render_widget(make_help_or_search(app), chunks[3]);
}

/// The A+B effort / finish-time line, shown just under the list.
fn make_summary(app: &App) -> Paragraph<'static> {
    match estimate_summary(&app.tasks, Local::now()) {
        Some(s) => Paragraph::new(Line::from(Span::styled(
            format!(" {s}"),
            Style::default().fg(Color::Cyan),
        ))),
        None => Paragraph::new(Line::from(Span::raw(""))),
    }
}

fn make_help_or_search(app: &App) -> Paragraph<'static> {
    if let Some(buf) = &app.search_input {
        return Paragraph::new(Line::from(vec![
            Span::raw(" /"),
            Span::raw(buf.clone()),
            Span::styled("▏", Style::default().fg(Color::Cyan)),
            Span::raw("  "),
            Span::styled(
                "Enter apply · Esc cancel",
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }
    if let Some(status) = &app.status {
        return Paragraph::new(Line::from(Span::styled(
            format!(" {status}"),
            Style::default().fg(Color::Yellow),
        )));
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
        " ↑↓ · 1-9 reorder · ⏎/e edit · a add · c done · d del · A/B/C cat · u/r undo · / search · Esc quit ",
        Style::default().fg(Color::DarkGray),
    ))
}

fn make_item(task: &Task, width: usize, selected: bool) -> ListItem<'static> {
    let category_char = task.category.to_string();
    let category_fg = match task.category {
        Category::A => Color::Red,
        Category::B => Color::Yellow,
        Category::C => Color::DarkGray,
    };

    let est_str = format_est(task.est_secs);

    // Cat column is right-aligned to CAT_W; the styled letter is one char wide,
    // so pad with CAT_W-1 leading spaces before the styled span.
    let cat_pad = " ".repeat(CAT_W.saturating_sub(1));
    let left = format!("   {:>w_id$}  {cat_pad}", task.id, w_id = ID_W);
    let middle = format!(
        "  {:<w_text$}  {:>w_est$}",
        truncate(&task.text, TEXT_W),
        est_str,
        w_text = TEXT_W,
        w_est = EST_W,
    );

    let mut row_chars = left.chars().count() + 1 + middle.chars().count();
    let mut trailing = String::new();
    while row_chars < width {
        trailing.push(' ');
        row_chars += 1;
    }

    let mut category_style = Style::default().fg(category_fg);
    if selected && task.category == Category::C {
        // The default C colour (DarkGray) clashes with the row-highlight bg (also
        // DarkGray), making the `C` invisible on the cursor row. Switch to a
        // high-contrast colour just for this case.
        category_style = Style::default().fg(Color::White);
    }

    let line_style = if selected {
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
