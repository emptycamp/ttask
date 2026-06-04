//! Reference-counted terminal screen setup, shared by every full-screen UI in the
//! app (the main `task` view, the task editor, and the link picker).
//!
//! Each of those used to enter and leave the alternate screen on its own. That is
//! fine when only one is on screen, but when one opens another — pressing `e`/`o` in
//! the main view to open the editor or link picker — the outer UI tore the screen
//! down and the inner one immediately rebuilt it, so the shell flashed into view for
//! a frame. Routing every entry/exit through this counter means the alternate screen
//! (and raw mode) are set up exactly once, on the outermost `enter`, and torn down
//! once, on the matching outermost `leave`. Nested calls are no-ops, so a UI opened
//! from inside another simply draws over the screen that is already up — no flicker.
//!
//! The app is single-threaded (one event loop), so the plain counter needs no
//! locking. Calls must be balanced; each `enter` pairs with exactly one `leave`.

use crate::error::{Error, Result};
use crossterm::cursor::Show;
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};

static DEPTH: AtomicUsize = AtomicUsize::new(0);

/// Enter raw mode + the alternate screen, unless we're already in it (a nested UI).
/// Pair every successful call with exactly one [`leave`].
pub fn enter() -> Result<()> {
    if DEPTH.load(Ordering::SeqCst) == 0 {
        enable_raw_mode().map_err(Error::Io)?;
        if let Err(e) = execute!(io::stdout(), EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(Error::Io(e));
        }
    }
    DEPTH.fetch_add(1, Ordering::SeqCst);
    Ok(())
}

/// Leave the alternate screen + raw mode, but only once the outermost UI exits.
/// A nested `leave` just drops the count, keeping the screen up for the UI that's
/// still using it. The cursor is restored when we finally return to the shell.
pub fn leave() {
    if DEPTH.fetch_sub(1, Ordering::SeqCst) == 1 {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
    }
}
