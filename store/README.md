# store

Local SQLite access. Reads and writes domain entities to the local database.

**Rules:**
- One file per domain entity
- Imports domain types; never imported by domain
- The only code that touches the database

**Lives here:** queries, inserts, upserts, migrations, connection setup.

## Database path

`store::status_cache::connect(profile)` uses `~/.hub/<profile>/hub.db`. The profile comes
from `HUB_PROFILE` via `config::profile::from_env()` and is `default` when unset, so the
installed hub reads `~/.hub/default/hub.db` and anything run through `just` reads
`~/.hub/dev/hub.db`. See [decision 024](../docs/decisions/024-hub-state-is-per-profile.md).

On first run after upgrading from an older build, `connect()` copies a database from an
older layout via `VACUUM INTO`, a WAL-safe one-time migration. Two layouts are tried,
newest first: `~/.hub/hub.db` (before profiles) and `~/Library/Application Support/hub/hub.db`
on macOS (before that). The source file is left in place, and later runs skip the migration
because the destination already exists.

**Only the `default` profile inherits.** Migrating into `dev` would copy the real cache into
the sandbox, which is the collision profiles exist to prevent, so `dev` always starts empty.

The profile directory is created automatically if absent. The `bundled` feature compiles
SQLite in — no system dependency.

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
