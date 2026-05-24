pub mod add;
pub mod clear;
pub mod complete;
pub mod delete;
pub mod edit;
pub mod history;
pub mod info;
pub mod list;

use crate::cli::{Cli, Cmd, HistoryCmd, OutputFormat};
use crate::clock::Clock;
use crate::confirm::Prompt;
use crate::editor::TaskEditor;
use crate::error::Result;
use crate::format::{sanitize_for_terminal, RenderOptions};
use crate::store::gc;
use crate::store::Store;
use crate::tui;

pub trait Tty {
    fn is_tty(&self) -> bool;
}

pub struct SystemTty;
impl Tty for SystemTty {
    fn is_tty(&self) -> bool {
        use std::io::IsTerminal;
        std::io::stdout().is_terminal()
    }
}

pub fn dispatch(
    cli: &Cli,
    store: &mut Store,
    clock: &dyn Clock,
    editor: &dyn TaskEditor,
    prompt: &dyn Prompt,
    tty: &dyn Tty,
) -> Result<()> {
    let gc_count = gc::sweep(store, clock)?;

    let md = matches!(cli.format, Some(OutputFormat::Md));
    let opts = if md {
        RenderOptions::markdown()
    } else if tty.is_tty() {
        RenderOptions::detect()
    } else {
        RenderOptions::no_color()
    };

    match &cli.cmd {
        None => {
            tui::run(store, clock, editor)?;
        }
        Some(Cmd::Add { args }) => {
            let task = add::run(args, store, clock)?;
            if md {
                print!("{}", crate::format::format_info_md(&task));
            } else {
                println!(
                    "Added task #{}: {}",
                    task.id,
                    sanitize_for_terminal(&task.text),
                );
            }
        }
        Some(Cmd::List {
            active,
            completed,
            deleted,
            all,
        }) => {
            let choice = list::resolve_filter(*active, *completed, *deleted, *all);
            let (output, _) = list::run_with_gc_count(store, choice, &opts, gc_count)?;
            let final_output = if md {
                list::format_with_footer_md(&output, gc_count)
            } else {
                list::format_with_footer(&output, gc_count)
            };
            print!("{final_output}");
        }
        Some(Cmd::Edit { id, args }) => {
            edit::run(*id, args, store, clock, editor)?;
            if md {
                let task = store.get_task(*id)?;
                print!("{}", crate::format::format_info_md(&task));
            } else {
                println!("Task #{id} updated.");
            }
        }
        Some(Cmd::Delete { id }) => {
            delete::run(*id, store, clock)?;
            if md {
                println!("**Deleted task #{id}.**");
            } else {
                println!("Task #{id} deleted.");
            }
        }
        Some(Cmd::Complete { id }) => {
            complete::run(*id, store, clock)?;
            if md {
                println!("**Completed task #{id}.**");
            } else {
                println!("Task #{id} completed.");
            }
        }
        Some(Cmd::Info { id }) => {
            let output = info::run(*id, store, &opts)?;
            print!("{output}");
        }
        Some(Cmd::Clear { yes }) => {
            let stats = clear::run(*yes, store, prompt)?;
            if md {
                println!(
                    "**Cleared {} task{} and {} history event{}.**",
                    stats.tasks_cleared,
                    if stats.tasks_cleared == 1 { "" } else { "s" },
                    stats.events_cleared,
                    if stats.events_cleared == 1 { "" } else { "s" },
                );
            } else {
                println!(
                    "Cleared {} task{} and {} history event{}.",
                    stats.tasks_cleared,
                    if stats.tasks_cleared == 1 { "" } else { "s" },
                    stats.events_cleared,
                    if stats.events_cleared == 1 { "" } else { "s" },
                );
            }
        }
        Some(Cmd::History { cmd, revert, yes }) => match revert {
            Some(id) => {
                let reverted = history::revert(*id, *yes, store, prompt)?;
                if md {
                    match reverted.len() {
                        1 => {
                            let (id, summary) = &reverted[0];
                            println!("**Reverted event #{id}:** {summary}");
                        }
                        n => {
                            println!("**Reverted {n} events** (newest first):\n");
                            for (id, summary) in &reverted {
                                println!("- #{id} — {summary}");
                            }
                        }
                    }
                } else {
                    match reverted.len() {
                        1 => {
                            let (id, summary) = &reverted[0];
                            println!("Reverted event #{id}: {summary}");
                        }
                        n => {
                            println!("Reverted {n} events (newest first):");
                            for (id, summary) in &reverted {
                                println!("  #{id}  {summary}");
                            }
                        }
                    }
                }
            }
            None => {
                // Default to interactive picker on a TTY; fall back to plain listing when
                // piped/captured (tests, scripts), when `list` is explicit, or in md mode.
                // -v / --verbose is only meaningful on the `list` subcommand; for the
                // non-tty / md fallbacks it stays off (minimal).
                let verbose = matches!(cmd, Some(HistoryCmd::List { verbose: true }));
                let explicit_list = matches!(cmd, Some(HistoryCmd::List { .. }));
                if md || explicit_list || !tty.is_tty() {
                    let output = history::list(store, &opts, verbose)?;
                    print!("{output}");
                } else {
                    crate::history_tui::run(store)?;
                }
            }
        },
    }
    Ok(())
}
