---
name: project-conventions
description: >
  Conventions for the `task` CLI codebase. Apply this skill whenever you:
  (1) add or modify a CLI subcommand / flag / output,
  (2) touch rendering in `src/format.rs`, `src/help_md.rs`, or the TUIs,
  (3) change the persistence schema (`src/store/**`, especially `RevertOp`),
  (4) write tests that depend on dates or "today",
  (5) edit help text or alias lists,
  (6) finish a change (build / fmt / git rules).
  These rules encode prior corrections — follow them exactly to avoid repeating
  past mistakes.
allowed-tools: Bash(cargo:*) Read Write Edit Glob Grep
---

# `task` Project Conventions

This skill is the single source of truth for project-specific conventions. The
rules below override generic instincts about CLI design, error handling, or test
ergonomics — they were each established by a prior correction.

## 1. Output is asymmetric: human-minimal, md-rich

The CLI has two audiences and they get different output from the same command.

**Human view (no `--format`)** must be **minimal**. No trailing footers, no
"hint" lines, no totals, no "tip:" verbiage. Truncation is silent except for a
tight inline marker on the affected row (`+N` chip on a day header). The
principle: a quick scan should fit on screen and not nag the user.

**Markdown view (`--format md`)** is **for LLM agents** and must be **rich**.
Include everything an agent needs to act without a follow-up query:

- Counts (visible / total) when truncation occurred.
- Explicit `(+N more)` on each truncated group heading.
- A `_Truncated: ..._` footer that names what's hidden (per-day rows AND hidden
  days), if anything was hidden.
- A second `_To see ... : `task list --active --format md`. ..._` footer that
  spells out the **exact commands** to reveal more — never just say "use a flag".
- Absolute timestamps (`YYYY-MM-DD HH:MM`) alongside relative ones, since the
  agent has no "now".

The same asymmetry applies to history: `task history list` is minimal (field
tokens only for edits — `text, p, due, est`); `--format md` is **always**
verbose (full per-field diff) regardless of any `-v` flag.

If you add a new command, ask: "what would an agent need to take the next step?"
and put that in the md output. Then ask: "what's the tightest thing that's still
useful?" for the human output. Never converge them.

## 2. `--format md` must cover every command — including `--help`

Every command must produce something sensible in md. `--help --format md` is the
trap: clap's auto-help short-circuits parsing, so dispatch never runs. The
existing solution: `src/main.rs` calls `help_md::wants_md_help(&raw_args)`
**before** `Cli::parse()` and routes to `help_md::render(path)` if both
`--help`/`-h` and `--format md`/`--format=md` are present, in any order.

When you add a new subcommand:

1. Add a `const FOO: &str = "..."` markdown block to `src/help_md.rs`.
2. Add the canonical name (and aliases) to `canonicalize()`.
3. Add a `["foo"] => FOO.to_string()` branch to `render()`.
4. Add an integration test in `tests/cli_help_md.rs`.

Do not skip this. An agent that types `task newthing --help --format md` and
gets clap's plain output is a regression.

## 3. Aliases are functional but hidden from `--help`

Use clap's `aliases = [...]` (and `alias = "..."` / `short_alias = '...'` for
args), **not** `visible_aliases` / `visible_alias` / `visible_short_alias`. The
aliases continue to work; they just don't clutter `--help`. Example examples in
`long_about` should use the canonical name only (no `task ls` examples on the
`list` long_about — write `task list`).

There is also no `task help` subcommand. The root `Cli` carries
`#[command(disable_help_subcommand = true)]` — keep it.

## 4. Compact list caps

In `src/format.rs`, `ListMode::Compact` is the implicit-default view for
`task list` with no filter flag. The caps are:

- `COMPACT_MAX_DAYS = 2` — at most today + the next day with tasks.
- `COMPACT_MAX_PER_DAY = 3` — at most 3 rows per visible day.

Full mode (`task list --active`, `--all`, etc.) does **not** cap. If you add a
new filter flag, make sure `resolve_filter` returns `explicit: true` so it
selects Full.

## 5. History schema: store enough to render diffs

`RevertOp` variants must carry enough state to render an informative log line
without re-reading the store:

- `Added { task: Task }` — full task (for `added #N: text`)
- `Edited { before: Task, after: Task }` — both sides (for the field diff)
- `Deleted { before: Task }` / `Completed { before: Task }` — the pre-state

If you change a variant, you change the on-disk format. bincode has zero
tolerance for added/changed fields, so old data won't decode. The migration
pattern: **rename the database**. Today it is `"history_v2"` (created in
`Store::open`). Bump to `"history_v3"` and let the old data sit untouched.
Don't try to be clever with `Option<NewField>` — bincode doesn't help you.

`RevertOp::summary()` is minimal-for-edits (`edited #1: text, p`).
`RevertOp::summary_verbose()` is the full diff (`text "old"→"new", p A→B`).
Revert confirmations and the "Reverted event #N: ..." line use
`summary_verbose()` — the user is acting on the change, so the detail is worth
it. The human history list uses `summary()` by default and switches to
`summary_verbose()` only with `-v`/`--verbose`. The md history always uses
`summary_verbose()`.

## 6. Date-sensitive tests must anchor on local "today"

`format_list` reads `Local::now().date_naive()` and rolls overdue *active* tasks
into the "Today" group via `effective_day`. A test using a fixed past date
constant (e.g. `2026-05-18`) will land all its tasks under "Today" once real
time moves past it, silently breaking the assertion you actually wanted to make.

When a test depends on a specific day relative to "now", use the helper:

```rust
fn at_local_noon(day_offset: i64) -> DateTime<Utc> { /* see src/format.rs tests */ }
```

Noon dodges DST + midnight rollover, and the local-day math is what
`effective_day` actually uses. There is also `due_at_local_noon(...)` in
`src/commands/list.rs::tests` for store-level tests. Reuse them; do **not**
re-anchor on `Utc::now() + Duration::days(N)` (timezone offset can land you on
an unexpected calendar day).

## 7. Code quality gates (always run before reporting done)

```
cargo fmt --check
cargo build      # must have NO code warnings (Permission-denied incremental
                 # warnings from the sandbox don't count)
cargo test       # all must pass
```

`cargo fmt` may reflow your test/code into something you didn't write. Run
`cargo fmt` then re-read changed files before claiming completion — there is
one prior incident where a formatter reflow combined with a partial Edit left
orphaned `}` and broken syntax. Spot-check the tail of any file you edited
after `cargo fmt`.

## 8. Sandbox + Git rules (from `CLAUDE.md`)

- **Network is filtered.** If a fetch fails, stop and report the URL — do not
  retry or look for workarounds.
- **Never touch the Git index.** No `git add`, `git reset`, `git restore
  --staged`. Reading state (`git status`, `git diff`) is fine. The user stages
  manually.
- **No `cargo clean`.** The target dir lives on a sandbox-permission boundary;
  removing it has produced a `Permission denied` failure that blocked further
  builds.

## 9. Conventions for new work

- Edit existing files; only create new modules when there's no natural home.
- New code follows the existing module layout: CLI in `cli.rs`, dispatch in
  `commands/mod.rs`, side-effects in `commands/<name>.rs`, rendering in
  `format.rs` (always two paths: human + md), persistence in `store/`.
- Default to **no comments**. Add a `// Why:` line only when the reason is
  non-obvious (a workaround, a subtle invariant, a hidden constraint).
- Don't add docs / READMEs unless explicitly asked.
- Tests live next to code (`#[cfg(test)] mod tests`) for unit tests; integration
  tests under `tests/cli_*.rs` use the `StoreScope` helper.
