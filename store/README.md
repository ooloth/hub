# store

Local SQLite access. Reads and writes domain entities to the local database.

**Rules:**
- One file per domain entity
- Imports domain types; never imported by domain
- The only code that touches the database

**Lives here:** queries, inserts, upserts, migrations, connection setup.
