use chrono::Utc;
use log::error;
use rss::{Category, Guid, Item};
use sqlx::SqlitePool;

pub struct SeenStore {
    pool: SqlitePool,
}

impl SeenStore {
    pub async fn new(db_path: &str) -> Result<Self, sqlx::Error> {
        let url = format!("sqlite://{}", db_path);
        let pool = SqlitePool::connect(&url).await?;
        let store = SeenStore { pool };
        store.init().await?;
        Ok(store)
    }

    async fn init(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS seen_ids (
                id TEXT PRIMARY KEY,
                first_seen TEXT NOT NULL
            );
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS items_archive (
                id TEXT PRIMARY KEY,
                title TEXT,
                link TEXT,
                description TEXT,
                author TEXT,
                categories TEXT,
                guid TEXT,
                pub_date TEXT NOT NULL,
                source_title TEXT,
                source_url TEXT,
                content TEXT,
                feed_source TEXT NOT NULL,
                archived_at TEXT NOT NULL
            );
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS feeds (
                feed TEXT PRIMARY KEY
            );
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_feeds(&self) -> Vec<String> {
        match sqlx::query_scalar::<_, String>("SELECT feed FROM feeds")
            .fetch_all(&self.pool)
            .await
        {
            Ok(list) => list,
            Err(e) => {
                error!("SeenStore::get_feeds error: {}", e);
                Vec::new()
            }
        }
    }

    pub async fn push_feeds(&self, feeds: Vec<String>) -> u64 {
        let mut inserted = 0;

        for feed in feeds {
            let res = sqlx::query(
                r#"
            INSERT INTO feeds (feed)
            VALUES (?1)
            ON CONFLICT(feed) DO NOTHING
            "#,
            )
            .bind(feed)
            .execute(&self.pool)
            .await;

            match res {
                Ok(done) => inserted += done.rows_affected(),
                Err(e) => {
                    error!("SeenStore::push_feeds error: {}", e);
                }
            }
        }

        inserted
    }

    pub async fn remove_feeds(&self, feeds: Vec<String>) -> u64 {
        let mut removed = 0;

        for feed in feeds {
            let res = sqlx::query(
                r#"
            DELETE FROM feeds
            WHERE feed = ?1
            "#,
            )
            .bind(feed)
            .execute(&self.pool)
            .await;

            match res {
                Ok(done) => removed += done.rows_affected(),
                Err(e) => {
                    error!("SeenStore::remove_feeds error: {}", e);
                }
            }
        }

        removed
    }

    pub async fn is_seen(&self, id: &str) -> bool {
        let res = sqlx::query("SELECT 1 FROM seen_ids WHERE id = ?1 LIMIT 1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await;

        match res {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(e) => {
                error!("SeenStore::is_seen error for id={}: {}", id, e);
                false
            }
        }
    }
    pub async fn mark_seen(&self, item: &Item, id: &str, feed_source: &str) -> bool {
        let title = item.title().map(|s| s.to_owned());
        let link = item.link().map(|s| s.to_owned());
        let description = item.description().map(|s| s.to_owned());
        let author = item.author().map(|s| s.to_owned());

        let categories_vec: Vec<String> = item
            .categories()
            .iter()
            .map(|c: &Category| c.name().to_owned())
            .collect();

        let categories_json = if categories_vec.is_empty() {
            None
        } else {
            match serde_json::to_string(&categories_vec) {
                Ok(s) => Some(s),
                Err(e) => {
                    error!(
                        "SeenStore::mark_seen: category JSON error for id={}: {}",
                        id, e
                    );
                    None
                }
            }
        };

        let guid_str = item.guid().map(|g: &Guid| g.value().to_owned());

        let pub_date = match item.pub_date() {
            Some(d) => d.to_owned(),
            None => Utc::now().to_rfc3339(),
        };

        let (source_title, source_url) = match item.source() {
            Some(src) => {
                let t = src.title().map(|s| s.to_owned());
                let u = Some(src.url().to_owned());
                (t, u)
            }
            None => (None, None),
        };

        let content = item.content().map(|s| s.to_owned());
        let now = Utc::now().to_rfc3339();

        let mut tx = match self.pool.begin().await {
            Ok(tx) => tx,
            Err(e) => {
                error!("SeenStore::mark_seen: cannot begin tx for id={}: {}", id, e);
                return false;
            }
        };

        let result = sqlx::query(
            r#"
            INSERT INTO seen_ids (id, first_seen)
            VALUES (?1, ?2)
            ON CONFLICT(id) DO NOTHING
            "#,
        )
        .bind(id)
        .bind(&now)
        .execute(&mut *tx)
        .await;

        let rows_affected = match result {
            Ok(r) => r.rows_affected(),
            Err(e) => {
                error!(
                    "SeenStore::mark_seen: insert into seen_ids failed for id={}: {}",
                    id, e
                );
                let _ = tx.rollback().await;
                return false;
            }
        };

        let archive_res = sqlx::query(
            r#"
            INSERT INTO items_archive (
                id,
                title,
                link,
                description,
                author,
                categories,
                guid,
                pub_date,
                source_title,
                source_url,
                content,
                feed_source,
                archived_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            ON CONFLICT(id) DO NOTHING
            "#,
        )
        .bind(id)
        .bind(title)
        .bind(link)
        .bind(description)
        .bind(author)
        .bind(categories_json)
        .bind(guid_str)
        .bind(pub_date)
        .bind(source_title)
        .bind(source_url)
        .bind(content)
        .bind(feed_source)
        .bind(now)
        .execute(&mut *tx)
        .await;

        if let Err(e) = archive_res {
            error!(
                "SeenStore::mark_seen: insert into items_archive failed for id={}: {}",
                id, e
            );
            let _ = tx.rollback().await;
            return false;
        }

        if let Err(e) = tx.commit().await {
            error!("SeenStore::mark_seen: commit failed for id={}: {}", id, e);
            return false;
        }

        rows_affected == 1
    }
}
