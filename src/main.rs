use std::io;

use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

mod demo;
mod imsa;
mod nls;
mod timing;
mod ui;

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn main() -> io::Result<()> {
    let dev_mode = std::env::args().any(|arg| arg == "--dev");

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
