# 003 — Config model: hub.toml for structure, .env for secrets

## Context

Hub runs on multiple devices with different projects active on each (work
laptop vs. personal laptop). As workflows multiply, per-project configuration
is needed: which repo to watch, which issue tracker to query, which Loki
labels to filter on. Environment variables handle credentials well but are a
poor fit for structured lists of projects and their properties.

## Decision

Two gitignored config files, each with a committed example:

**`.env`** — credentials only. One token per service type, shared across all
projects that use that service. Secrets are stored in 1Password and injected
at runtime via `op run --env-file=.env`. Never contains project-specific
structure.

**`hub.toml`** — structure only. No secrets. Lists projects and the workflows
active for each, plus a `[monitor]` section for non-project observations:

```toml
[[project]]
name = "hub"
repo = "ooloth/hub"
workflows = ["github-prs", "github-issues"]

[[project]]
name = "work-api"
repo = "company/work-api"
workflows = ["github-prs", "jira-tickets", "gke-pod-health", "loki-errors"]

[monitor]
workflows = ["home-server-health"]
```

Workflows are listed **explicitly** per project and in `[monitor]`. Hub runs
exactly what is listed — no inference from project properties, no implicit
defaults. Projects vary too widely (a static site vs. a GKE deployment) for
a useful default set; opt-out lists for complex projects would be long and
hard to mentally compute. Explicit opt-in keeps each project's config
self-contained and immediately readable.

`[monitor]` uses the same `workflows` key as projects. Non-project
observations (home server health, calendar, etc.) are just workflows without
a codebase behind them — no reason to model them differently.

Each device has its own `hub.toml` listing only the projects relevant to that
machine. When hub-private is in use, per-device configs live in
`hub-private/devices/<name>.toml` and are symlinked to the hub root.

## Consequences

- The `config` crate will read both files: parse `hub.toml` for structure,
  read env vars for credentials. Everything downstream receives a single typed
  `Config` struct. Currently only env var loading is implemented; `hub.toml`
  parsing is next.
- `hub.toml.example` and `.env.example` are committed to the repo and kept
  up to date as workflows are added.
- A project listed in `hub.toml` whose required env vars are absent produces
  no items — clean degradation when a service is unconfigured.
- Per-workflow config (e.g. polling cadence) is a future extension; the
  `workflows` list will grow to support inline options when needed.
