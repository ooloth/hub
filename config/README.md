# config

Reads env vars into typed domain structs. The single place where env var names are known.

**Rules:**
- Only imported by `ui/cli` and `ui/tui` — never by workflows, clients, or store
- Inner layers receive config values as arguments; they never reach out for them
- Never reads files — secrets arrive as env vars, injected by `op run` before the process starts

**Lives here:** `std::env::var` calls, validation (are required vars present?), conversion into typed structs.

**Secrets** are managed in 1Password and injected at runtime:
```bash
op run --env-file=.env -- just <recipe>
```
