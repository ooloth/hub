# config

Parses `hub.toml` and resolves credentials into typed structs. The single
place where config file structure and credential sourcing are known.

**Rules:**
- Only `ui/tui` and `daemon` have this crate as a Rust dependency — workflows, clients, store, and
  `ui/cli` never import it
- Inner layers receive config values as function arguments; they do not call back into config
- Secrets are wrapped in `Secret<String>` (secrecy crate); `.expose_secret()` is called only at client call sites

**Lives here:** `hub.toml` parsing, async credential resolution, `validate_required()`,
typed config structs, and the JSON schemas used to validate `hub.toml` in editors (`schemas/`).
Credential resolution starts `op` itself: `op_read.rs` reads one credential under its deadline and
falls back to an `op` that answers for itself, `op_process.rs` starts `op` either way, and
`screen_look.rs` runs `osascript` and `ps` to see who was not answering at a deadline.

**Credentials** live in `hub.toml`'s `[credentials]` table. Values starting with
`op://` are resolved at startup via `op read`; plain strings pass through unchanged.
See `docs/architecture/secrets.md` for the full model.
