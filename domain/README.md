# domain

Types and pure business logic. The shared vocabulary of the system.

**Rules:**
- No I/O, no network calls, no file reads
- No imports from other hub crates
- Everything else imports from here; nothing here imports upward

**Lives here:** domain entities (PR, Error, Item), config schema (plain structs), pure functions (urgency scoring, age calculation).
