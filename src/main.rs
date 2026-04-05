use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use deli::app::App;
use deli::models::config::AppConfig;

#[derive(Debug, Parser)]
#[command(name = "deli", about = "Local-first DevOps TUI console")]
struct Cli {
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    workspace: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = AppConfig::load(cli.config.as_deref())?;
    let mut app = App::new(config, cli.workspace)?;
    app.run()
}
