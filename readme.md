# korvatunturi-rss

A lightweight RSS watcher daemon that keeps track of feed items, stores them in SQLite, and emits notifications through a webhook endpoint (e.g., Discord). The project is intentionally modular so you can swap out how feed events are processed without rewriting the fetching or persistence layers.

## Features
- Periodically polls multiple RSS feeds concurrently using Tokio.
- Persists seen item metadata to a SQLite database to avoid duplicates.
- Optional webhook delivery for new items with embed payloads suitable for chat platforms.
- Local socket control interface for adding or removing feeds while the daemon is running.
- CLI helper to validate feed URLs before running the service.

## Configuration
Settings are loaded from a TOML file (default `config.toml`) and can be overridden with environment variables prefixed with `APP_`.

```toml
[feeds]
# Inline feed URLs. You can also provide a newline-delimited list via `file_path`.
list = ["https://example.com/feed.xml"]
# Optional path to a file containing additional feed URLs
file_path = "feeds.txt"
# Maximum number of queued events waiting for processing
queue = 128
# Refresh interval in seconds
refresh_interval = 900

[database]
# SQLite file path
path = "./data/rss.db"

# Webhook endpoint for new items (Discord-compatible by default)
webhook = "https://discord.com/api/webhooks/<id>/<token>"

# Local socket path or name used for CLI commands
socket = "rssd.sock"
```

### Environment overrides
Environment variables mirror the config structure using `APP_` and underscores:

- `APP_FEEDS_LIST` for a comma-separated list of feed URLs
- `APP_FEEDS_QUEUE` for the queue size
- `APP_DATABASE_PATH` for the SQLite file location
- `APP_WEBHOOK` for the webhook URL
- `APP_SOCKET` for the socket name

## Usage
Install Rust and run the daemon:

```bash
cargo run --release -- --daemon
```

### Validate feed URLs
Check that configured feeds resolve correctly before launching:

```bash
cargo run --release -- --check
# Increase verbosity for full feed lists
cargo run --release -- --check -vv
```

### Runtime commands
Commands are sent over the configured local socket:

```bash
# Add a feed while the daemon is running
cargo run --release -- --cli feed add https://example.com/feed.xml

# Remove a feed
cargo run --release -- --cli feed remove https://example.com/feed.xml

# Health check and version
cargo run --release -- --cli ping
cargo run --release -- --cli version
```

## Extending behavior
The webhook payload is built in `src/server/server.rs` inside `handle_event`, where feed events arrive after being deduplicated and archived. Adjust that function or swap in alternative handlers to forward items to other services while reusing the existing fetching, scheduling, and storage components.
