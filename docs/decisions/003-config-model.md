# 003 — Config model: hub.toml for structure, .env for secrets

## Context

Hub runs on multiple devices with different integrations (work laptop vs
personal laptop). As integrations multiply, per-project configuration is
needed: which repos to watch, which Loki service labels, which Linear
projects. Environment variables handle credentials well but are a poor
fit for structured lists of projects.

## Decision

Two gitignored config files, each with a committed example:

**`.env`** — credentials only. One token per service type, shared across
all projects that use that service. Secrets are stored in 1Password and
injected at runtime via `op run --env-file=.env`. Never contains
project-specific structure.

**`hub.toml`** — structure only. No secrets. Defines which projects exist
on this device, which integrations each uses, and any integration-specific
identifiers (service labels, repo slugs, etc.):

```toml
context = "personal"   # or "work"

[[projects]]
name = "hub"
repo = "ooloth/hub"
integrations = ["github-prs", "loki-errors"]
loki_service = "hub"

[[projects]]
name = "blog"
repo = "ooloth/blog"
integrations = ["github-prs"]
```

Both files live at the hub repo root (gitignored). Work laptop has its own
`hub.toml` listing work projects; personal laptop lists personal projects.
The code is identical on both devices.

Integrations are enabled by the presence of their required env vars. If
a service URL or token isn't in `.env`, that integration silently skips.
No explicit enable/disable list required.

## Consequences

- The `config` crate reads both files: parses `hub.toml` for structure,
  reads env vars for credentials. Everything downstream receives a single
  typed `Config` struct.
- `hub.toml.example` and `.env.example` are committed to the repo and kept
  up to date as integrations are added.
- `hub.toml` and `.env` can be stored in `hub-private` and symlinked to
  the hub root, giving them backup and version control without exposing
  them publicly.
- A project entry with no active env vars for its integrations produces no
  items — clean degradation when a service is unavailable or unconfigured.
