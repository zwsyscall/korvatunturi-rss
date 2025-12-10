use crate::{
    config::AppConfig,
    feeds::watcher::{FeedEvent, RssManager},
    server::commands::ServerCommand,
};
use std::time::Duration;

use log::{debug, error, info, warn};
use reqwest::Client;
use serde_json::json;
use tokio::{
    select,
    sync::mpsc::{self, Receiver},
};
use {
    interprocess::local_socket::{
        GenericNamespaced, ListenerOptions,
        tokio::{Stream, prelude::*},
    },
    tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
};

async fn handle_event(event: FeedEvent, webhook: Option<&str>, client: &Client) {
    let title = event
        .item
        .title
        .as_deref()
        .unwrap_or("<title not specified>");
    let description = event
        .item
        .description
        .as_deref()
        .unwrap_or("<description not specified>");
    let link = event.item.link.as_deref().unwrap_or("<link not specified>");

    debug!("Event: [{}] {} => {}", event.source, title, link);
    if let Some(url) = webhook {
        let payload = json!({
            "content": "",
            "tts": false,
            "embeds": [
                {
                    "title": title,
                    "description": description,
                    "url": link,
                    "color": 4963212
                }
            ]
        });

        if let Err(e) = client.post(url).json(&payload).send().await {
            error!("Error sending alert: {}", e);
        }
    }
}

pub async fn start(cfg: AppConfig) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!("Starting RSS watcher");
    let feeds = cfg.feeds.get();
    let (mut manager, failed_urls) = RssManager::new(
        &cfg.database.path,
        &feeds,
        cfg.feeds.queue,
        // Might? panic
        Duration::from_secs(cfg.feeds.refresh_interval as u64),
    )
    .await?;

    info!("RssManager initialized with {} feeds", manager.len());
    if !failed_urls.is_empty() {
        warn!(
            "{} feeds failed to initialize. Please run --check -vv to identify them",
            failed_urls.len()
        );
    }
    let mut command_recv = create_ipc_listener(&cfg.socket)?;
    let client = Client::new();
    // Main loop
    loop {
        select! {
            maybe_event = manager.next() => {
                // Main logic of what to do with events goes here
                if let Some(e) = maybe_event {
                    handle_event(e, cfg.webhook.as_deref(), &client).await;
                }
            }
            cmd = command_recv.recv() => {
                if let Some(command) = cmd {
                    match command {
                        ServerCommand::AddFeed(feed) => {
                            if let Err(e) = manager.add_feed(&feed).await {
                                error!("Error adding feed: {:?}", e);
                                continue;
                            }
                            info!("Added {} feed", feed)
                        },
                        ServerCommand::RemoveFeed(feed) => {
                            if !manager.remove_feed(&feed).await {
                                error!("Feed does not exist");
                                continue;
                            }
                            info!("Removed {} feed", feed)
                        },
                        // Branches are handled in the listener
                        _ => warn!("This branch should never be hit"),
                    }
                }
            }

        }
    }
}

fn create_ipc_listener(
    socket_name: &str,
) -> Result<Receiver<ServerCommand>, Box<dyn std::error::Error + Send + Sync>> {
    let name = socket_name.to_ns_name::<GenericNamespaced>()?;
    let listener = ListenerOptions::new().name(name).create_tokio()?;
    let (send, recv) = mpsc::channel(300);

    info!("Listening for commands on {}", socket_name);
    tokio::spawn(async move {
        loop {
            let conn = match listener.accept().await {
                Ok(c) => c,
                Err(e) => {
                    error!("Error creating listener: {}", e);
                    continue;
                }
            };
            let tx = send.clone();

            tokio::spawn(async move {
                if let Some(cmd) = parse_message(conn).await {
                    if tx.send(cmd).await.is_err() {
                        error!("Error sending command")
                    }
                }
            });
        }
    });

    return Ok(recv);
}

async fn parse_message(conn: Stream) -> Option<ServerCommand> {
    let mut recver = BufReader::new(&conn);
    let mut sender = &conn;
    let mut buffer = String::with_capacity(2048);

    if recver.read_line(&mut buffer).await.ok()? == 0 {
        return None;
    }
    debug!("Client sent: {}", &buffer);

    match ServerCommand::try_from(buffer.trim().to_string()) {
        Ok(cmd) => {
            if let Some(reply) = cmd.format_reply() {
                // Send the correct reply
                if sender.write_all(reply.as_bytes()).await.is_err() {
                    error!("Failed to send reply");
                }
                None
            } else {
                // Default to ACK
                if sender.write_all(b"ACK").await.is_err() {
                    error!("Failed to send ACK");
                }
                Some(cmd)
            }
        }
        Err(e) => {
            error!("Error converting buffer to command: {:?}", e);
            None
        }
    }
}
