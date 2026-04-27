# store

Local SQLite access. Reads and writes domain entities to the local database.

**Rules:**
- One file per domain entity
- Imports domain types; never imported by domain
- The only code that touches the database

**Lives here:** queries, inserts, upserts, migrations, connection setup.

## SQLite (rusqlite)

```toml
rusqlite = { version = "0.31", features = ["bundled"] }
```

`bundled` compiles SQLite in — no system dependency.

```rust
let conn = Connection::open(&db_path)?;
conn.execute("INSERT INTO items (title) VALUES (?1)", [&title])?;
let count: i64 = conn.query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0))?;
```

Upgrade to `sqlx` if async DB access becomes necessary.
