use rss::{Channel, Item};
use sha2::{Digest, Sha256};
use std::collections::{HashSet, VecDeque};

use crate::db::SeenStore;

#[derive(Clone)]
pub struct RssFeed {
    source: String,
    seen_items: HashSet<String>,
    seen_order: VecDeque<String>,
    items: Vec<Item>,
    max_cache: usize,
}

impl RssFeed {
    pub async fn new(
        url: String,
        max_size: usize,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let content = reqwest::get(&url).await?.bytes().await?;
        let mut channel = Channel::read_from(&content[..])?;
        channel.set_link(&url);

        Ok(Self {
            source: url,
            seen_items: HashSet::new(),
            seen_order: VecDeque::new(),
            items: Vec::new(),
            max_cache: max_size,
        })
    }

    pub fn source(&self) -> String {
        self.source.clone()
    }

    pub fn items(&mut self) -> Vec<Item> {
        std::mem::take(&mut self.items)
    }

    fn remember(&mut self, id: String) {
        if self.seen_items.contains(&id) {
            return;
        }

        if self.seen_order.len() >= self.max_cache {
            if let Some(old_id) = self.seen_order.pop_front() {
                self.seen_items.remove(&old_id);
            }
        }

        self.seen_order.push_back(id.clone());
        self.seen_items.insert(id);
    }

    pub async fn refresh(
        &mut self,
        store: &SeenStore,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let content = reqwest::get(&self.source).await?.bytes().await?;

        let channel = Channel::read_from(&content[..])?;
        for item in channel.into_items() {
            let id = item_hash(&item);

            // In memory route
            if self.seen_items.contains(&id) {
                continue;
            }

            // Check backing Db
            if store.is_seen(&id).await {
                self.remember(id);
                continue;
            }

            // Add to database
            store.mark_seen(&item, &id, &self.source).await;
            self.remember(id.clone());
            self.items.push(item);
        }
        Ok(())
    }
}

fn item_hash(item: &Item) -> String {
    if let Some(guid) = item.guid() {
        return guid.value().to_string();
    }

    // fallback
    let mut hasher = Sha256::new();
    if let Some(link) = item.link() {
        hasher.update(link.as_bytes());
    }
    if let Some(title) = item.title() {
        hasher.update(title.as_bytes());
    }
    if let Some(desc) = item.description() {
        hasher.update(desc.as_bytes());
    }

    hex::encode(hasher.finalize())
}
