# store

Local SQLite access. Reads and writes domain entities to the local database.

**Rules:**
- One file per domain entity
- Imports domain types; never imported by domain
- The only code that touches the database

**Lives here:** queries, inserts, upserts, migrations, connection setup.

## Database path

`store::status::connect()` resolves the path in this order:

1. `HUB_DB_PATH` env var (set to any absolute path to override)
2. Platform default via `dirs::data_local_dir()`:
   - macOS: `~/Library/Application Support/hub/hub.db`
   - Linux: `~/.local/share/hub/hub.db`

The parent directory is created automatically on first run. The `bundled`
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

`rusqlite::Connection` is not `Send` — do not share it across async tasks.
Open a new connection per operation, or keep it in one task and pass results
over a channel.

Upgrade to `sqlx` if async DB access becomes necessary.
