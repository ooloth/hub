# store

Local SQLite access. Reads and writes domain entities to the local database.

**Rules:**
- One file per domain entity
- Imports domain types; never imported by domain
- The only code that touches the database

**Lives here:** queries, inserts, upserts, migrations, connection setup.

## Database path

`store::status_cache::connect()` always uses `~/.hub/hub.db`.

On first run after upgrading from an older build, `connect()` automatically copies the
legacy platform path (`~/Library/Application Support/hub/hub.db` on macOS) to the new
location via `VACUUM INTO` — a WAL-safe, one-time migration. The legacy file is left in
place. Subsequent runs skip the migration because the new path already exists.

The parent directory (`~/.hub/`) is created automatically if absent. The `bundled`
feature compiles SQLite in — no system dependency.

## SQLite (rusqlite)

```toml
rusqlite = { version = "0.31", features = ["bundled"] }
```

```rust
let conn = Connection::open(&db_path)?;
conn.execute("INSERT INTO items (title) VALUES (?1)", [&title])?;
let count: i64 = conn.query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0))?;
```

`rusqlite::Connection` is not `Send` — it cannot be moved across tokio task
boundaries. You can use it freely within a single task (including an async
`#[tokio::main]` function). The TUI pattern is correct: keep the connection
in the main task and send results over an `mpsc` channel from spawned tasks.

Upgrade to `sqlx` if async DB access becomes necessary.
