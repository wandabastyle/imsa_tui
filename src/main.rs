use std::io;

use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

#[cfg(feature = "dev-mode")]
mod demo;
mod imsa;
mod nls;
mod timing;
mod ui;

#[derive(Debug, Default)]
struct Args {
    #[cfg(feature = "dev-mode")]
    dev: bool,
}

impl Args {
    fn parse() -> Self {
        let mut args = Self::default();
        #[cfg(feature = "dev-mode")]
        {
            args.dev = std::env::args().any(|arg| arg == "--dev");
        }
        args
    }
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn main() -> io::Result<()> {
    let args = Args::parse();
    let dev_mode = {
        #[cfg(feature = "dev-mode")]
        {
            args.dev
        }

        #[cfg(not(feature = "dev-mode"))]
        {
            false
        }
    };

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let app_result = ui::run_app(&mut terminal, dev_mode);
    let restore_result = restore_terminal(&mut terminal);

    match (app_result, restore_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(app_err), Ok(())) => Err(app_err),
        (Ok(()), Err(restore_err)) => Err(restore_err),
        (Err(app_err), Err(_)) => Err(app_err),
    }
}
