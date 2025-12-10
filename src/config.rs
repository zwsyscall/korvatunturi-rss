use config::{Config, Environment, File};
use serde::Deserialize;
use std::io::{self, BufRead};

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub feeds: Feeds,
    pub database: Database,
    pub socket: String,
    pub webhook: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Database {
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct Feeds {
    pub list: Vec<String>,
    pub file_path: Option<String>,
    pub queue: usize,
    pub refresh_interval: usize,
}

pub fn load_config(path: &str) -> Result<AppConfig, config::ConfigError> {
    let builder = Config::builder()
        .add_source(File::with_name(path))
        .add_source(Environment::with_prefix("APP").separator("_"));
    let cfg = builder.build()?.try_deserialize::<AppConfig>()?;
    Ok(cfg)
}

impl Feeds {
    pub fn get(&self) -> Vec<String> {
        let mut feed_list = self.list.clone();
        if let Some(path) = &self.file_path {
            if let Ok(file) = std::fs::File::open(path) {
                let feeds: Vec<String> = io::BufReader::new(file)
                    .lines()
                    .filter_map(|l| l.ok())
                    .filter(|l| !l.trim().is_empty())
                    .collect();
                feed_list.extend(feeds);
            }
        }
        feed_list
    }
}
