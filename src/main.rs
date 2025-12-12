mod args;
mod cli;
mod config;
mod db;
mod feeds;
mod server;
use clap::Parser;
use colored::Colorize;
use env_logger;
use log::error;

use crate::server::ServerCommand;

fn init_logging(v: u8) {
    let filter = match v {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(filter)).init();
}

#[tokio::main]
async fn main() {
    let args = args::Args::parse();

    // Config
    let cfg = match config::load_config(&args.config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to load config {}: {}", args.config, e);
            return;
        }
    };

    if args.check {
        cli::check_feeds(cfg.feeds.get(), args.verbose).await;
        return;
    }

    if args.daemon {
        init_logging(args.verbose);
        if let Err(e) = server::start(cfg).await {
            error!("Starting daemon failed: {:?}", e);
        }
        return;
    }

    if !args.cli.is_empty() {
        let arg = args.cli.join(" ");
        match ServerCommand::try_from(arg) {
            Ok(cmd) => {
                if let Err(e) = cli::send_command(cfg, cmd).await {
                    eprintln!("{} {:?}", "Sending command failed:".red().bold(), e);
                }
            }
            Err(e) => {
                eprintln!("{} {}", "Error parsing command:".red().bold(), e);
            }
        }
    }
}
