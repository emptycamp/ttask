//! `task open <id>` — find link(s) in a task's text and open one with the system's
//! default handler. With several links, an interactive picker chooses; passing a
//! 1-based index (`task open <id> 2`) skips the picker, which keeps the command
//! usable from scripts and non-interactive contexts.

use crate::commands::Tty;
use crate::error::{Error, Result};
use crate::model::TaskId;
use crate::store::Store;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::{Frame, Terminal};
use std::io;

/// Open a link from task `id`. Returns the URL that was opened, or `None` if an
/// interactive picker was cancelled.
pub fn run(
    id: TaskId,
    index: Option<usize>,
    store: &Store,
    tty: &dyn Tty,
) -> Result<Option<String>> {
    let task = store.get_task(id)?;
    let links = extract_links(&task.text);
    match select_link(&links, index, tty.is_tty()) {
        Selection::None => Err(Error::Parse(format!("task #{id} contains no links"))),
        Selection::OutOfRange(count) => Err(Error::Parse(format!(
            "link {} is out of range; task #{id} has {count} link{}",
            index.unwrap_or(0),
            if count == 1 { "" } else { "s" },
        ))),
        Selection::Ambiguous => Err(Error::Parse(ambiguous_message(id, &links))),
        Selection::Open(url) => {
            launch(&url)?;
            Ok(Some(url))
        }
        Selection::Picker => match pick_link(&links)? {
            Some(url) => {
                launch(&url)?;
                Ok(Some(url))
            }
            None => Ok(None),
        },
    }
}

/// The outcome of deciding which link to act on, independent of any I/O so it can be
/// unit-tested directly.
#[derive(Debug, PartialEq, Eq)]
enum Selection {
    Open(String),
    Picker,
    None,
    /// Index given but past the end; carries the available link count.
    OutOfRange(usize),
    /// Several links, no index, and no TTY for a picker.
    Ambiguous,
}

fn select_link(links: &[String], index: Option<usize>, is_tty: bool) -> Selection {
    if links.is_empty() {
        return Selection::None;
    }
    if let Some(n) = index {
        if n == 0 || n > links.len() {
            return Selection::OutOfRange(links.len());
        }
        return Selection::Open(links[n - 1].clone());
    }
    if links.len() == 1 {
        return Selection::Open(links[0].clone());
    }
    if is_tty {
        Selection::Picker
    } else {
        Selection::Ambiguous
    }
}

fn ambiguous_message(id: TaskId, links: &[String]) -> String {
    let mut msg = format!(
        "task #{id} has {} links; pass the link number (e.g. `task open {id} 1`):",
        links.len(),
    );
    for (i, l) in links.iter().enumerate() {
        msg.push_str(&format!("\n  {}. {l}", i + 1));
    }
    msg
}

/// Pull every URL out of `text`, in order, de-duplicated. Recognises `http://`,
/// `https://`, and a leading `www.` (rewritten to `https://`). Surrounding brackets,
/// quotes, and trailing sentence punctuation are stripped.
pub fn extract_links(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in text.split_whitespace() {
        if let Some(url) = link_in_token(token) {
            if !out.contains(&url) {
                out.push(url);
            }
        }
    }
    out
}

fn link_in_token(token: &str) -> Option<String> {
    // The link may be embedded (e.g. a markdown `](https://…)`), so search for the
    // earliest scheme-ish start within the token.
    let start = ["http://", "https://", "www."]
        .iter()
        .filter_map(|p| token.find(p))
        .min()?;
    let candidate = token[start..].trim_end_matches(|c: char| ")]}>\"'`.,;:!?".contains(c));
    let url = if candidate.starts_with("http://") || candidate.starts_with("https://") {
        candidate.to_string()
    } else {
        format!("https://{candidate}")
    };
    if is_meaningful_url(&url) {
        Some(url)
    } else {
        None
    }
}

/// A URL is worth offering only if it has a plausible host after the scheme — a
/// non-empty host with a dot (a domain) or a colon (an explicit port).
fn is_meaningful_url(url: &str) -> bool {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"));
    match rest {
        Some(rest) => {
            let host = rest.split(['/', '?', '#']).next().unwrap_or("");
            host.len() > 1 && (host.contains('.') || host.contains(':'))
        }
        None => false,
    }
}

/// Hand `url` to the OS default handler. Spawned detached and never waited on, so the
/// launcher's exit code is irrelevant and the terminal is never blocked.
/// `TASK_OPEN_DRY_RUN` short-circuits the spawn — used by tests and as an escape hatch
/// for non-interactive callers.
fn launch(url: &str) -> Result<()> {
    if std::env::var("TASK_OPEN_DRY_RUN").is_ok() {
        return Ok(());
    }
    opener_command(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(Error::Io)?;
    Ok(())
}

fn opener_command(url: &str) -> std::process::Command {
    let (program, args) = opener_spec(std::env::consts::OS, url);
    let mut cmd = std::process::Command::new(program);
    cmd.args(args);
    cmd
}

/// The program + args to open `url` with the OS default handler. On Windows we use
/// `rundll32 url.dll,FileProtocolHandler <url>` rather than `explorer <url>`:
/// `explorer` treats a URL's `?query` as a filename wildcard and pops a File Explorer
/// window instead of handing the link to the browser. `rundll32` passes the whole URL
/// straight to the protocol handler with no shell parsing, so query strings work.
fn opener_spec(os: &str, url: &str) -> (&'static str, Vec<String>) {
    match os {
        "windows" => (
            "rundll32.exe",
            vec!["url.dll,FileProtocolHandler".to_string(), url.to_string()],
        ),
        "macos" => ("open", vec![url.to_string()]),
        _ => ("xdg-open", vec![url.to_string()]),
    }
}

fn pick_link(links: &[String]) -> Result<Option<String>> {
    // Reuse the main TUI's alternate screen when the picker is opened from inside it
    // (the `o` key), so there's no leave/re-enter flicker. `enter`/`leave` are
    // balanced regardless of how `pick_on_screen` returns.
    crate::screen::enter()?;
    let result = pick_on_screen(links);
    crate::screen::leave();
    result
}

fn pick_on_screen(links: &[String]) -> Result<Option<String>> {
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).map_err(Error::Io)?;
    // Shared alternate screen (see `screen`): clear whatever the main TUI drew before
    // painting the picker, otherwise this terminal's diff skips the picker's blank cells
    // and the list underneath shows through.
    terminal.clear().map_err(Error::Io)?;
    pick_loop(&mut terminal, links)
}

fn pick_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    links: &[String],
) -> Result<Option<String>> {
    let mut cursor = 0usize;
    loop {
        terminal
            .draw(|f| draw_picker(f, links, cursor))
            .map_err(Error::Io)?;
        let key = match event::read().map_err(Error::Io)? {
            Event::Key(k) if k.kind == KeyEventKind::Press => k,
            _ => continue,
        };
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => return Ok(None),
            (KeyCode::Up, _) => cursor = cursor.saturating_sub(1),
            (KeyCode::Down, _) => cursor = (cursor + 1).min(links.len().saturating_sub(1)),
            (KeyCode::Enter, _) => return Ok(links.get(cursor).cloned()),
            (KeyCode::Char(c), _) if c.is_ascii_digit() && c != '0' => {
                let n = (c as usize) - ('0' as usize);
                if let Some(url) = links.get(n - 1) {
                    return Ok(Some(url.clone()));
                }
            }
            _ => {}
        }
    }
}

fn draw_picker(frame: &mut Frame, links: &[String], cursor: usize) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // title
            Constraint::Min(3),    // links
            Constraint::Length(1), // help
        ])
        .split(frame.area());

    frame.render_widget(
        Paragraph::new(Span::styled(
            " Open which link?",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        chunks[0],
    );

    let items: Vec<ListItem> = links
        .iter()
        .enumerate()
        .map(|(i, l)| ListItem::new(Line::from(Span::raw(format!(" {}. {l}", i + 1)))))
        .collect();
    let mut state = ListState::default();
    state.select(Some(cursor));
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );
    ratatui::widgets::StatefulWidget::render(list, chunks[1], frame.buffer_mut(), &mut state);

    frame.render_widget(
        Paragraph::new(Span::styled(
            " ↑↓ select · 1-9 quick pick · Enter open · Esc cancel ",
            Style::default().fg(Color::DarkGray),
        )),
        chunks[2],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_single_http_link() {
        assert_eq!(
            extract_links("see https://example.com for details"),
            vec!["https://example.com"]
        );
    }

    #[test]
    fn extracts_multiple_links_in_order() {
        let links = extract_links("a http://one.com b https://two.org/path c");
        assert_eq!(links, vec!["http://one.com", "https://two.org/path"]);
    }

    #[test]
    fn rewrites_bare_www_to_https() {
        assert_eq!(
            extract_links("visit www.rust-lang.org today"),
            vec!["https://www.rust-lang.org"]
        );
    }

    #[test]
    fn strips_surrounding_punctuation_and_brackets() {
        assert_eq!(
            extract_links("ticket (https://jira.example.com/T-1)."),
            vec!["https://jira.example.com/T-1"]
        );
    }

    #[test]
    fn finds_link_embedded_in_markdown() {
        assert_eq!(
            extract_links("[docs](https://docs.rs/task)"),
            vec!["https://docs.rs/task"]
        );
    }

    #[test]
    fn de_duplicates_repeated_links() {
        assert_eq!(
            extract_links("https://x.com and again https://x.com"),
            vec!["https://x.com"]
        );
    }

    #[test]
    fn ignores_non_links_and_bare_schemes() {
        assert!(extract_links("just some plain text").is_empty());
        assert!(extract_links("http:// https:// www.").is_empty());
        assert!(extract_links("email me at a@b.com").is_empty());
    }

    #[test]
    fn extracts_links_across_newlines() {
        let links = extract_links("line one https://a.com\nline two https://b.com");
        assert_eq!(links, vec!["https://a.com", "https://b.com"]);
    }

    #[test]
    fn select_none_when_no_links() {
        assert_eq!(select_link(&[], None, true), Selection::None);
    }

    #[test]
    fn select_single_link_opens_without_picker() {
        let links = vec!["https://a.com".to_string()];
        assert_eq!(
            select_link(&links, None, true),
            Selection::Open("https://a.com".into())
        );
    }

    #[test]
    fn select_index_picks_that_link() {
        let links = vec!["https://a.com".to_string(), "https://b.com".to_string()];
        assert_eq!(
            select_link(&links, Some(2), false),
            Selection::Open("https://b.com".into())
        );
    }

    #[test]
    fn select_index_out_of_range() {
        let links = vec!["https://a.com".to_string()];
        assert_eq!(select_link(&links, Some(5), true), Selection::OutOfRange(1));
        assert_eq!(select_link(&links, Some(0), true), Selection::OutOfRange(1));
    }

    #[test]
    fn select_multiple_uses_picker_on_tty() {
        let links = vec!["https://a.com".to_string(), "https://b.com".to_string()];
        assert_eq!(select_link(&links, None, true), Selection::Picker);
    }

    #[test]
    fn select_multiple_is_ambiguous_without_tty() {
        let links = vec!["https://a.com".to_string(), "https://b.com".to_string()];
        assert_eq!(select_link(&links, None, false), Selection::Ambiguous);
    }

    #[test]
    fn extracts_youtube_url_with_query_string() {
        // The query string must survive — this is the link that broke the opener.
        assert_eq!(
            extract_links("watch https://www.youtube.com/watch?v=KVUj4LrlPEU now"),
            vec!["https://www.youtube.com/watch?v=KVUj4LrlPEU"]
        );
    }

    #[test]
    fn opener_spec_windows_uses_url_protocol_handler_not_explorer() {
        let url = "https://www.youtube.com/watch?v=KVUj4LrlPEU";
        let (program, args) = opener_spec("windows", url);
        assert_eq!(program, "rundll32.exe");
        assert_eq!(
            args,
            vec!["url.dll,FileProtocolHandler".to_string(), url.to_string()]
        );
    }

    #[test]
    fn opener_spec_macos_uses_open() {
        let (program, args) = opener_spec("macos", "https://x.com");
        assert_eq!(program, "open");
        assert_eq!(args, vec!["https://x.com".to_string()]);
    }

    #[test]
    fn opener_spec_other_oses_use_xdg_open() {
        assert_eq!(opener_spec("linux", "https://x.com").0, "xdg-open");
        assert_eq!(opener_spec("freebsd", "https://x.com").0, "xdg-open");
    }
}
