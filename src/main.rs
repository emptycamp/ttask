use clap::Parser;
use task::cli::Cli;
use task::clock::SystemClock;
use task::commands::{dispatch, SystemTty};
use task::confirm::StdinPrompt;
use task::editor::BuiltinEditor;
use task::help_md;
use task::store::Store;

fn main() {
    let raw_args: Vec<String> = std::env::args().collect();
    // Intercept `--help --format md` before clap's auto-help short-circuits. We can't
    // do this from inside the normal dispatch because clap exits as soon as it sees
    // `--help`, never reaching our handlers.
    if help_md::wants_md_help(&raw_args) {
        let path = help_md::extract_subcommand_path(&raw_args);
        print!("{}", help_md::render(&path));
        return;
    }

    let cli = Cli::parse();

    // Restore terminal on panic (important for TUI / form editor)
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(std::io::stderr(), crossterm::terminal::LeaveAlternateScreen);
        hook(info);
    }));

    let db_path = Store::default_path(cli.test);
    let mut store = match Store::open(&db_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    let clock = SystemClock;
    let editor = BuiltinEditor;
    let prompt = StdinPrompt;
    let tty = SystemTty;

    if let Err(e) = dispatch(&cli, &mut store, &clock, &editor, &prompt, &tty) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
