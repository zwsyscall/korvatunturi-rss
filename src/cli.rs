use crate::{config::AppConfig, feeds::watcher::resolve_feeds, server::ServerCommand};
use colored::*;
use spinners::{Spinner, Spinners};
use std::io;
use tokio::io::AsyncReadExt;
use {
    interprocess::local_socket::{
        GenericNamespaced,
        tokio::{Stream, prelude::*},
    },
    tokio::io::AsyncWriteExt,
};

pub async fn check_feeds(feeds: Vec<String>, v: u8) {
    let mut sp = Spinner::new(Spinners::Dots, "Checking feeds".blue().bold().to_string());
    let (feeds, failed_feeds) = resolve_feeds(feeds).await;
    sp.stop();

    let succesful_feeds: Vec<String> = feeds.iter().map(|f| f.source()).collect();
    let succeed = succesful_feeds.len();
    let failed = failed_feeds.len();

    println!(
        "\n{} {}\n{} {}\n{} {}/{}",
        "Valid feeds:".green().bold().underline(),
        succeed,
        "Invalid feeds:".red().bold().underline(),
        failed,
        "Valid feed rate:".bold().underline(),
        succeed,
        succeed + failed
    );

    if v > 1 {
        println!(
            "{}\n{}",
            "Succesful feeds".green().bold().underline(),
            succesful_feeds.join("\n")
        );
        println!(
            "{}\n{}",
            "Failed feeds".red().bold().underline(),
            failed_feeds.join("\n")
        );
    }
}

pub async fn send_command(cfg: AppConfig, command: ServerCommand) -> io::Result<()> {
    let name = cfg.socket.to_ns_name::<GenericNamespaced>()?;
    let mut conn = Stream::connect(name).await?;
    let cmd = command.to_string();

    conn.write_all(cmd.as_bytes()).await?;
    if !cmd.ends_with('\n') {
        conn.write_all(b"\n").await?;
    }
    conn.flush().await?;

    let mut buffer = String::with_capacity(2048);
    conn.read_to_string(&mut buffer).await?;

    let result = match &buffer[..4] {
        "ACK " => "Ok: ".green().bold(),
        "ERR " => "Error: ".red().bold(),
        _ => "Unknown: ".yellow().bold(),
    };
    println!("{} {}", result, &buffer[4..]);

    Ok(())
}
