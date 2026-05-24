use crate::model::TaskId;
use clap::{ArgGroup, Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(name = "task", about = "Personal task manager", version = env!("CARGO_PKG_VERSION"))]
#[command(disable_help_subcommand = true)]
pub struct Cli {
    #[arg(long, global = true, hide = true)]
    pub test: bool,
    /// Output format. `md` produces output optimized for LLM agents.
    #[arg(long, global = true, value_enum, value_name = "FORMAT")]
    pub format: Option<OutputFormat>,
    #[command(subcommand)]
    pub cmd: Option<Cmd>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// Markdown output optimized for LLM agents.
    Md,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// Add a new task.
    #[command(aliases = ["create", "new"])]
    #[command(long_about = "\
Add a new task.

Examples:
  task add Buy milk
  task add Read book p:a due:tomorrow est:1h
  task add Plan sprint due:jun15 est:2h p:b
  task add \"Quick chore\" p:c

Fields you can set inline:
  p:a | p:b | p:c                priority
  due:tomorrow | due:fri | ...   when it's due (keyword, weekday, MMMd, duration)
  est:30m | est:1h               estimated effort
")]
    Add { args: Vec<String> },
    /// List tasks. By default shows only active tasks.
    #[command(aliases = ["ls"])]
    #[command(group(ArgGroup::new("filter").args(["active", "completed", "deleted", "all"]).multiple(false)))]
    #[command(long_about = "\
List tasks. By default shows only active tasks.

Examples:
  task list                       # active tasks (default)
  task list --active              # active tasks (explicit)
  task list -a                    # short for --active
  task list --completed           # only completed
  task list --deleted             # only soft-deleted
  task list --all                 # everything (active + completed + deleted)
")]
    List {
        /// Show only active tasks (default).
        #[arg(short = 'a', long, aliases = ["activeonly", "open", "pending", "todo"])]
        active: bool,
        /// Show only completed tasks.
        #[arg(long, aliases = ["complete", "done", "finished", "finish", "closed", "completes"])]
        completed: bool,
        /// Show only deleted tasks.
        #[arg(long, aliases = ["delete", "deletes", "removed", "remove", "removes", "trash", "trashed", "discarded"])]
        deleted: bool,
        /// Show all tasks (active + completed + deleted).
        #[arg(long, aliases = ["every", "everything"])]
        all: bool,
    },
    /// Edit an existing task. With no field args, opens the built-in form editor.
    #[command(aliases = ["update", "modify", "change", "set"])]
    #[command(long_about = "\
Edit an existing task.

With no field args, opens the built-in form editor inside the terminal. The form
editor requires a real TTY; in scripts or piped contexts, pass field args
(p:/due:/est:/text) directly.

Examples:
  task edit 3                       # open the form editor in this terminal
  task edit 3 p:a                   # set priority via args (scriptable)
  task edit 3 New text              # change text via args
  task edit 3 due:tomorrow est:30m
")]
    Edit { id: TaskId, args: Vec<String> },
    /// Delete a task (soft delete; can be reverted via history).
    #[command(aliases = ["del", "remove", "rm", "discard", "trash"])]
    #[command(long_about = "\
Delete a task. The task is soft-deleted and can be restored from history.

Examples:
  task delete 3
")]
    Delete { id: TaskId },
    /// Mark a task as completed.
    #[command(aliases = ["done", "finish", "finished", "close"])]
    #[command(long_about = "\
Mark a task as completed.

Examples:
  task complete 3
")]
    Complete { id: TaskId },
    /// Show full task details.
    #[command(aliases = ["show", "view", "details"])]
    #[command(long_about = "\
Show full task details (text, priority, due, est, status, timestamps).

Examples:
  task info 3
")]
    Info { id: TaskId },
    /// Wipe the entire database — every task and every history event.
    #[command(aliases = ["wipe", "nuke", "reset"])]
    #[command(long_about = "\
Wipe the entire database — every task and every history event. This cannot be
undone. By default you get a confirmation prompt; pass -y / -f to skip it.

Examples:
  task clear                       # confirm, then wipe
  task clear -y                    # no prompt
  task clear --force               # equivalent to -y
")]
    Clear {
        /// Skip the confirmation prompt.
        #[arg(short = 'y', long, short_alias = 'f', alias = "force")]
        yes: bool,
    },
    /// Show recent change history, or revert a specific event by ID.
    #[command(aliases = ["log", "events"])]
    #[command(long_about = "\
Show recent change history, or revert a specific event by ID.

Running `task history` with no subcommand opens an interactive picker so you can
choose which event to undo. Use `task history list` to dump events to stdout
instead. By default edits show only the names of changed fields; pass `-v` to
include the full old→new diff.

The last 30 events are kept. Each event has a stable ID you can revert.

Examples:
  task history                          # interactive picker (Enter to undo)
  task history list                     # plain stdout list (minimal)
  task history list -v                  # include old→new diffs for edits
  task history --revert 12              # revert event #12 (with confirmation)
  task history --revert 12 -y           # skip confirmation
  task history --revert 12 --force      # equivalent to -y
")]
    History {
        #[command(subcommand)]
        cmd: Option<HistoryCmd>,
        /// Revert the change with this history ID.
        #[arg(long, aliases = ["undo", "rollback"], value_name = "ID")]
        revert: Option<u64>,
        /// Skip the confirmation prompt.
        #[arg(short = 'y', long, short_alias = 'f', alias = "force")]
        yes: bool,
    },
}

#[derive(Subcommand)]
pub enum HistoryCmd {
    /// Print history events to stdout (no interactive picker).
    #[command(aliases = ["ls"])]
    List {
        /// Include detailed old→new values for edits. Without this, edits only show
        /// the names of the fields that changed.
        #[arg(short = 'v', long)]
        verbose: bool,
    },
}
