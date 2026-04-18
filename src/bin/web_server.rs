use imsa_tui::web::{daemon, server};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mode = daemon::parse_mode()?;
    if daemon::handle_lifecycle_mode(mode)? {
        return Ok(());
    }
    server::run(mode).await
}
