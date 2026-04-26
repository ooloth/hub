# config

Parses `~/.hub/config.toml` into typed domain structs.

**Rules:**
- Only imported by `ui/cli` and `ui/tui` — never by workflows, clients, or store
- Inner layers receive config values as arguments; they never reach out for them

**Lives here:** file reading, TOML deserialization, env var overrides, validation.
