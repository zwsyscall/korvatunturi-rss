use std::{collections::HashMap, sync::Arc, time::Duration};

use futures::future::join_all;
use log::{debug, error, trace};
use rss::Item;
use tokio::sync::{
    mpsc::{self, Receiver, Sender},
    oneshot,
};

use crate::{db::SeenStore, feeds::feed::RssFeed};

pub struct FeedEvent {
    pub source: String,
    pub item: Item,
}

pub struct RssManager {
    event_sender: Sender<FeedEvent>,
    events: Receiver<FeedEvent>,
    feed_list: HashMap<String, oneshot::Sender<()>>,
    normal_sleep: Duration,
    fail_sleep: Duration,
    seen_store: Arc<SeenStore>,
}

impl RssManager {
    pub async fn new(
        database_path: &str,
        rss_feeds: &[String],
        queue_size: usize,
        sleep_interval: Duration,
    ) -> Result<(Self, Vec<String>), sqlx::Error> {
        let fail_sleep = std::time::Duration::from_secs(60 * 60);
        let (send, recv) = mpsc::channel(queue_size);
        let db = SeenStore::new(database_path).await?;

        // --------- FEED SETUP ---------
        // Fetch feeds from database so that we can push new feeds as we want
        let mut feed_list = db.get_feeds().await;
        feed_list.extend_from_slice(rss_feeds);

        let (feeds, failed_urls) = resolve_feeds(feed_list).await;

        // Sync database with feeds
        db.push_feeds(feeds.iter().map(|f| f.source()).collect())
            .await;

        // --------- READING SETUP ---------
        let seen_mutex = Arc::new(db);
        let mut feed_list = HashMap::new();
        // Clone every single feed and run their synching in tasks to get rid of as much blocking as possible
        // Blocking will still occur when they use the SeenStore
        for feed in feeds {
            // Creates a URL => oneshot sender entry in the feed_list
            feed_list.insert(
                feed.source().to_string(),
                feed_refresh_loop(
                    send.clone(),
                    Arc::clone(&seen_mutex),
                    feed,
                    sleep_interval,
                    fail_sleep,
                ),
            );
        }

        Ok((
            Self {
                event_sender: send,
                events: recv,
                normal_sleep: sleep_interval,
                fail_sleep: fail_sleep,
                seen_store: seen_mutex,
                feed_list: feed_list,
            },
            failed_urls,
        ))
    }

    pub async fn add_feed(
        &mut self,
        url: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let feed = RssFeed::new(url.to_string(), 300).await?;
        self.seen_store.push_feeds(vec![feed.source()]).await;

        self.feed_list.insert(
            feed.source(),
            feed_refresh_loop(
                self.event_sender.clone(),
                Arc::clone(&self.seen_store),
                feed,
                self.normal_sleep.clone(),
                self.fail_sleep.clone(),
            ),
        );

        Ok(())
    }

    // Maybe remove it from the feeds thing too? idk
    pub async fn remove_feed(&mut self, url: &str) -> bool {
        self.seen_store.remove_feeds(vec![url.to_string()]).await;

        if let Some(oneshot) = self.feed_list.remove(url) {
            debug!("Found feed {}", url);
            if let Err(e) = oneshot.send(()) {
                error!("Error sending oneshot to quit: {:?}", e);
            }
            return true;
        }
        debug!("Did not find feed {}", url);
        return false;
    }

    pub async fn next(&mut self) -> Option<FeedEvent> {
        self.events.recv().await
    }

    pub fn len(&self) -> usize {
        self.feed_list.len()
    }
}

pub async fn resolve_feeds(feeds: Vec<String>) -> (Vec<RssFeed>, Vec<String>) {
    let feed_futs = feeds.iter().map(|url| {
        let url = (*url).to_string();
        async move {
            let result = RssFeed::new(url.clone(), 300).await;
            (url, result)
        }
    });
    let results: Vec<(String, Result<RssFeed, _>)> = join_all(feed_futs).await;
    let feeds: Vec<RssFeed> = results
        .iter()
        .filter_map(|(_, res)| res.as_ref().ok().cloned())
        .collect();

    let failed_urls: Vec<String> = results
        .into_iter()
        .filter_map(|(url, res)| if res.is_err() { Some(url) } else { None })
        .collect();
    return (feeds, failed_urls);
}

fn feed_refresh_loop(
    tx: Sender<FeedEvent>,
    store: Arc<SeenStore>,
    mut feed: RssFeed,
    normal_sleep: Duration,
    fail_sleep: Duration,
) -> oneshot::Sender<()> {
    let (sender, mut quit_recv) = oneshot::channel();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut quit_recv => {
                    trace!("Quit signal received for feed {}", feed.source());
                    break;
                }

                _ = refresh_once(&tx, &*store, &mut feed, normal_sleep, fail_sleep) => {
                }
            }
        }
    });
    sender
}

async fn refresh_once(
    tx: &Sender<FeedEvent>,
    store: &SeenStore,
    feed: &mut RssFeed,
    normal_sleep: Duration,
    fail_sleep: Duration,
) {
    trace!("Starting to refresh feed {}", feed.source());
    let start = tokio::time::Instant::now();

    if let Err(e) = feed.refresh(store).await {
        error!("Error refreshing {}: {:?}", feed.source(), e);
        tokio::time::sleep(fail_sleep).await;
        return;
    }

    for item in feed.items() {
        if tx
            .send(FeedEvent {
                source: feed.source(),
                item,
            })
            .await
            .is_err()
        {
            return;
        }
    }

    let elapsed = start.elapsed();
    trace!("Feed {} took {:.3?} to refresh", feed.source(), elapsed);
    if elapsed < normal_sleep {
        tokio::time::sleep(normal_sleep - elapsed).await;
    }
}
