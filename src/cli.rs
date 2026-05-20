use crate::model::TaskId;
use clap::{ArgGroup, Parser, Subcommand};

#[derive(Parser)]
#[command(name = "task", about = "Personal task manager", version = env!("CARGO_PKG_VERSION"))]
pub struct Cli {
    #[arg(long, global = true, hide = true)]
    pub test: bool,
    #[command(subcommand)]
    pub cmd: Option<Cmd>,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// Add a new task.
    #[command(visible_aliases = ["create", "new"])]
    #[command(long_about = "\
Add a new task.

Examples:
  task add Buy milk
  task create Read book p:a due:tomorrow est:1h
  task add Plan sprint due:jun15 est:2h p:b
  task new \"Quick chore\" p:c

Fields you can set inline:
  p:a | p:b | p:c                priority
  due:tomorrow | due:fri | ...   when it's due (keyword, weekday, MMMd, duration)
  est:30m | est:1h               estimated effort
")]
    Add {
        args: Vec<String>,
    },
    /// List tasks. By default shows only active tasks.
    #[command(visible_aliases = ["ls"])]
    #[command(group(ArgGroup::new("filter").args(["active", "completed", "deleted", "all"]).multiple(false)))]
    #[command(long_about = "\
List tasks. By default shows only active tasks.

Examples:
  task list                       # active tasks (default)
  task ls                         # short alias
  task list --active              # active tasks (explicit)
  task list -a                    # short for --active
  task list --completed           # only completed
  task list --done                # alias of --completed
  task list --finished            # alias of --completed
  task list --deleted             # only soft-deleted
  task list --trash               # alias of --deleted
  task list --removed             # alias of --deleted
  task list --all                 # everything (active + completed + deleted)
")]
    List {
        /// Show only active tasks (default).
        #[arg(short = 'a', long, visible_aliases = ["activeonly", "open", "pending", "todo"])]
        active: bool,
        /// Show only completed tasks.
        #[arg(long, visible_aliases = ["complete", "done", "finished", "finish", "closed", "completes"])]
        completed: bool,
        /// Show only deleted tasks.
        #[arg(long, visible_aliases = ["delete", "deletes", "removed", "remove", "removes", "trash", "trashed", "discarded"])]
        deleted: bool,
        /// Show all tasks (active + completed + deleted).
        #[arg(long, visible_aliases = ["every", "everything"])]
        all: bool,
    },
    /// Edit an existing task. With no field args, opens the built-in form editor.
    #[command(visible_aliases = ["update", "modify", "change", "set"])]
    #[command(long_about = "\
Edit an existing task.

With no field args, opens the built-in form editor inside the terminal. The form
editor requires a real TTY; in scripts or piped contexts, pass field args
(p:/due:/est:/text) directly.

Examples:
  task edit 3                       # open the form editor in this terminal
  task edit 3 p:a                   # set priority via args (scriptable)
  task edit 3 New text              # change text via args
  task update 3 due:tomorrow est:30m
  task modify 3 p:c                 # alias
")]
    Edit {
        id: TaskId,
        args: Vec<String>,
    },
    /// Delete a task (soft delete; can be reverted via history).
    #[command(visible_aliases = ["del", "remove", "rm", "discard", "trash"])]
    #[command(long_about = "\
Delete a task. The task is soft-deleted and can be restored from history.

Examples:
  task delete 3
  task del 3
  task rm 3
  task remove 3
  task discard 3
  task trash 3
")]
    Delete {
        id: TaskId,
    },
    /// Mark a task as completed.
    #[command(visible_aliases = ["done", "finish", "finished", "close"])]
    #[command(long_about = "\
Mark a task as completed.

Examples:
  task complete 3
  task done 3
  task finish 3
  task close 3
")]
    Complete {
        id: TaskId,
    },
    /// Show full task details.
    #[command(visible_aliases = ["show", "view", "details"])]
    #[command(long_about = "\
Show full task details (text, priority, due, est, status, timestamps).

Examples:
  task info 3
  task show 3
  task view 3
  task details 3
")]
    Info {
        id: TaskId,
    },
    /// Wipe the entire database — every task and every history event.
    #[command(visible_aliases = ["wipe", "nuke", "reset"])]
    #[command(long_about = "\
Wipe the entire database — every task and every history event. This cannot be
undone. By default you get a confirmation prompt; pass -y / -f to skip it.

Examples:
  task clear                       # confirm, then wipe
  task wipe                        # alias
  task clear -y                    # no prompt
  task clear --force               # alias of -y
")]
    Clear {
        /// Skip the confirmation prompt.
        #[arg(short = 'y', long, visible_short_alias = 'f', visible_alias = "force")]
        yes: bool,
    },
    /// Show recent change history, or revert a specific event by ID.
    #[command(visible_aliases = ["log", "events"])]
    #[command(long_about = "\
Show recent change history, or revert a specific event by ID.

Running `task history` with no flags opens an interactive picker so you can choose
which event to undo. Use --list to dump events to stdout instead.

The last 30 events are kept. Each event has a stable ID you can revert.

Examples:
  task history                          # interactive picker (Enter to undo)
  task history --list                   # plain stdout list
  task log --list                       # alias
  task events --list                    # alias
  task history --revert 12              # revert event #12 (with confirmation)
  task history --revert 12 -y           # skip confirmation
  task history --revert 12 -f           # alias of -y
  task history --revert 12 --force      # alias of -y
  task history --undo 12                # alias of --revert
")]
    History {
        /// Print history events to stdout (no interactive picker).
        #[arg(long, visible_aliases = ["ls"])]
        list: bool,
        /// Revert the change with this history ID.
        #[arg(long, visible_aliases = ["undo", "rollback"], value_name = "ID")]
        revert: Option<u64>,
        /// Skip the confirmation prompt.
        #[arg(short = 'y', long, visible_short_alias = 'f', visible_alias = "force")]
        yes: bool,
    },
}
