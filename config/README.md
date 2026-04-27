# config

Reads env vars and `hub.toml` into typed structs. The single place where
env var names and config file structure are known.

**Rules:**
- Only imported by `ui/cli` and `ui/tui` — never by workflows, clients, or store
- Inner layers receive config values as arguments; they never reach out for them
- Never reads secrets from files — secrets arrive as env vars, injected by `op run`

**Lives here:** `std::env::var` calls, `hub.toml` parsing, validation, typed config structs,
and the JSON schemas used to validate `hub.toml` in editors (`schemas/`).

**Secrets** are managed in 1Password and injected at runtime:
```bash
op run --env-file=.env -- just <recipe>
```
