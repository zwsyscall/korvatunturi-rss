use crate::{
    config::AppConfig,
    feeds::watcher::{FeedEvent, RssManager},
    reply_err, reply_ok,
    server::commands::{CommandMessage, ServerCommand},
};
use std::time::Duration;

use log::{debug, error, info, warn};
use reqwest::Client;
use serde_json::json;
use tokio::{
    select,
    sync::{
        mpsc::{self},
        oneshot,
    },
};
use {
    interprocess::local_socket::{
        GenericNamespaced, ListenerOptions,
        tokio::{Stream, prelude::*},
    },
    tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
};

// For now this is just using discord. This is mainly a placeholder function
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
    info!("Starting RSS watcher server");
    let feeds = cfg.feeds.get();
    let (mut manager, failed_urls) = RssManager::new(
        &cfg.database.path,
        &feeds,
        cfg.feeds.queue,
        Duration::from_secs(cfg.feeds.refresh_interval.try_into()?),
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
    loop {
        select! {
            maybe_event = manager.next() => {
                if let Some(e) = maybe_event {
                    handle_event(e, cfg.webhook.as_deref(), &client).await;
                }
            }
            cmd = command_recv.recv() => {
                if let Some(CommandMessage { cmd, reply: tx }) = cmd {
                    match cmd {
                        ServerCommand::AddFeed(feed) => {
                            match manager.add_feed(&feed).await {
                                Ok(new) => {
                                    let msg = if new { "Added" } else { "Did not add" };
                                    reply_ok!(tx, "{} feed: {}", msg, feed);
                                }
                                Err(e) => {
                                    reply_err!(tx, "Could not add feed: {:?}", e);
                                    continue;
                                }
                            }
                        },

                        ServerCommand::RemoveFeed(feed) => {
                            if !manager.remove_feed(&feed).await {
                                reply_err!(tx, "Feed is not being followed");
                                continue;
                            }
                            reply_ok!(tx, "Removed {} feed", feed);
                        },

                        ServerCommand::GetFeeds => {
                            let feeds = manager.feeds().join(", ");
                            reply_ok!(tx, "Returning feeds: {}", &feeds)
                        },

                        _ => {
                            if let Some(msg) = cmd.format_reply() {
                                reply_ok!(tx, "{}", msg);
                                continue;
                            }
                            reply_ok!(tx, "No reply");
                        }
                    }
                }
            }

        }
    }
}

fn create_ipc_listener(
    socket_name: &str,
) -> Result<mpsc::Receiver<CommandMessage>, Box<dyn std::error::Error + Send + Sync>> {
    let name = socket_name.to_ns_name::<GenericNamespaced>()?;
    let listener = ListenerOptions::new().name(name).create_tokio()?;
    let (command_send, command_recv) = mpsc::channel(300);

    info!("Listening for commands on {}", socket_name);
    tokio::spawn(async move {
        loop {
            let conn = match listener.accept().await {
                Ok(c) => c,
                Err(e) => {
                    error!("Error accepting connection: {}", e);
                    continue;
                }
            };

            let tx = command_send.clone();
            tokio::spawn(handle_connection(conn, tx));
        }
    });

    Ok(command_recv)
}

async fn handle_connection(conn: Stream, command_tx: mpsc::Sender<CommandMessage>) {
    let mut recver = BufReader::new(&conn);
    let mut sender = &conn;
    let mut buffer = String::with_capacity(2048);

    // Reads input from the client
    buffer.clear();
    match recver.read_line(&mut buffer).await {
        Ok(0) => {
            debug!("Client disconnected");
            return;
        }
        Ok(_) => {}
        Err(e) => {
            error!("Failed to read from client: {}", e);
            return;
        }
    }
    debug!("Client sent: {}", &buffer.trim());

    // Parse input
    let cmd = match ServerCommand::try_from(buffer.trim().to_string()) {
        Ok(c) => c,
        Err(e) => {
            error!("Error converting buffer to command: {:?}", e);
            let _ = sender.write_all(b"ERR invalid command\n").await;
            return;
        }
    };

    // Send upstream
    let (reply_tx, reply_rx) = oneshot::channel::<String>();
    if command_tx
        .send(CommandMessage {
            cmd,
            reply: reply_tx,
        })
        .await
        .is_err()
    {
        error!("Failed to forward command to server");
        let _ = sender.write_all(b"ERR internal\n").await;
        return;
    }

    // Waits for a reply from the upstream server
    match reply_rx.await {
        Ok(reply) => {
            if let Err(e) = sender.write_all(reply.as_bytes()).await {
                error!("Failed to send reply to client: {}", e);
            }
        }
        Err(_canceled) => {
            error!("Reply channel dropped before sending response");
            let _ = sender.write_all(b"ERR no-reply\n").await;
        }
    }
}
