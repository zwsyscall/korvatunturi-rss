use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "rssd")]
#[command(about = "RSS daemon", version)]
pub struct Args {
    /// Path to config file
    #[arg(short, long, default_value = "config.toml")]
    pub config: String,

    /// Increase log verbosity
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Run in daemon mode
    #[arg(long)]
    pub daemon: bool,

    /// Check if currently added feeds are valid
    #[arg(long)]
    pub check: bool,

    // For communicating with a running daemon
    #[arg(long, num_args = 1..)]
    pub cli: Vec<String>,
}
