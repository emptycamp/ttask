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
Add a new task. With no arguments, opens the built-in editor (same as pressing `a`
in the `task` view); type the task text and Esc to save.

Examples:
  task add                       # open the editor for a new task
  task add Buy milk
  task add Buy milk 30m
  task add Read book c:a est:1h
  task add Plan sprint c:b est:2h ord:1
  task add \"Quick chore\" c:c

Fields you can set inline:
  c:a | c:b | c:c                category (A, B, or C; `p:` works as a legacy alias)
  ord:N                          manual order (1-based; insert at position N within the category)
  est:30m | est:1h               estimated effort
  30m | 4.5h (bare token)        a duration at the start/end of the text sets the
                                 estimate too (est: wins if both are given)

Order is tracked per-category: each of A, B, and C has its own 1-based sequence.

Auto-deletion by category (working days only, Mon–Fri; weekends are skipped):
  A    never auto-deleted
  B    auto-deleted after 1 work week (5 working days) without an update
  C    auto-deleted after 2 working days without an update
Editing the task (text, category, ord, est) resets the clock.
")]
    Add { args: Vec<String> },
    /// List tasks. By default shows only active tasks.
    #[command(aliases = ["ls"])]
    #[command(group(ArgGroup::new("filter").args(["active", "completed", "deleted", "all"]).multiple(false)))]
    #[command(long_about = "\
List tasks. By default shows only active tasks, grouped by category (A, then B,
then C) and ordered within each category by its manual order.

Given a task ID, shows that single task's full details instead — `task list 3` is a
shortcut for `task info 3`.

Examples:
  task list                       # active tasks (default)
  task list 3                     # full details for task #3 (like `task info 3`)
  task list --active              # active tasks (explicit)
  task list -a                    # short for --active
  task list --completed           # only completed
  task list --deleted             # only soft-deleted
  task list --all                 # everything (active + completed + deleted)
")]
    List {
        /// Show this task's full details instead of the list (a shortcut for
        /// `task info <id>`). When omitted, the normal list is shown.
        id: Option<TaskId>,
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
    #[command(aliases = ["e", "update", "modify", "change", "set"])]
    #[command(long_about = "\
Edit an existing task.

With no field args, opens the built-in text editor inside the terminal: a small
text area pre-filled with the task text. Enter inserts a newline (tasks may carry a
multi-line description); Esc saves and exits. A duration token at the end of the
text (e.g. `Buy milk 45m`) sets the estimate,
including on a multi-line description; on a single-line task a leading token works
too. Category and ord are changed from the main `task` view or via
args, not in the editor. The editor requires a real TTY; in scripts or piped
contexts, pass field args (c:/ord:/est:/text) directly.

Examples:
  task edit 3                       # open the text editor in this terminal
  task edit 3 c:a                   # set category via args (scriptable)
  task edit 3 New text              # change text via args
  task edit 3 New text 45m          # change text and set estimate (bare token)
  task edit 3 ord:1 est:30m         # move to first position and update estimate
")]
    Edit { id: TaskId, args: Vec<String> },
    /// Delete a task (soft delete; can be reverted via history).
    #[command(aliases = ["del", "remove", "rm", "discard", "trash"])]
    #[command(long_about = "\
Delete a task. The task is soft-deleted and can be restored from history.

Soft-deleted tasks are hard-removed automatically 1 work week (5 working days)
after deletion.

Examples:
  task delete 3
")]
    Delete { id: TaskId },
    /// Mark a task as completed.
    #[command(aliases = ["done", "finish", "finished", "close"])]
    #[command(long_about = "\
Mark a task as completed.

Completed tasks are hard-removed automatically 1 work week (5 working days)
after completion.

Examples:
  task complete 3
")]
    Complete { id: TaskId },
    /// Show full task details.
    #[command(aliases = ["show", "view", "details"])]
    #[command(long_about = "\
Show full task details (text, category, ord, est, status, timestamps).

Examples:
  task info 3
")]
    Info { id: TaskId },
    /// Open a link found in a task's text.
    #[command(aliases = ["o", "launch"])]
    #[command(long_about = "\
Open a link contained in a task's text using the system's default handler
(the default browser on Windows, `open` on macOS, `xdg-open` on Linux).

`task open <ID>` scans the task text for URLs (http://, https://, or a leading
`www.`):
  no links           an error
  exactly one link   it is opened
  several links      a picker lets you choose one — or pass the link number
                     directly to skip it: `task open <ID> 2`

Examples:
  task open 3                     # open the only link in task #3 (or pick one)
  task open 3 2                   # open the 2nd link in task #3 (no picker)
")]
    Open {
        /// Task to open a link from.
        id: TaskId,
        /// Which link to open (1-based) when the task has several — skips the picker.
        index: Option<usize>,
    },
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
