# 003 — Config model: hub.toml for structure, .env for secrets

## Context

Hub runs on multiple devices with different projects active on each (work
laptop vs. personal laptop). As workflows multiply, per-project configuration
is needed: which repo to watch, which issue tracker to query, which log
labels to filter on. Environment variables handle credentials well but are a
poor fit for structured lists of projects and their properties.

## Decision

Two gitignored config files, each with a committed example:

**`.env`** — credentials only. One token per service type, shared across all
projects that use that service. Secrets are stored in 1Password and injected
at runtime via `op run --env-file=.env`. Never contains project-specific
structure.

**`hub.toml`** — structure only. No secrets. Has two top-level concepts:
`[[project]]` for codebases, and `[monitor]` for non-project observations.

### Projects

Each `[[project]]` has a `name`, a `repo` (`owner/name`), and workflows.
Workflows are always objects — there is no string shorthand. `name` is the
only required field; all other fields are workflow-defined.

Projects that have no deployment concept (config repos, static sites, tools)
list workflows directly under `[[project.workflow]]`:

```toml
[[project]]
name = "config-nvim"
repo = "ooloth/config-nvim"

[[project.workflow]]
name = "github-prs"
```

Projects that deploy to one or more environments use `[[project.environment]]`.
Each environment carries the platform context its workflows need (`gcp_project`,
`service`, etc.) and lists its own workflows under `[[project.environment.workflow]]`:

```toml
[[project]]
name = "my-app"
repo = "company/my-app"

[[project.workflow]]
name = "github-prs"

[[project.environment]]
env = "prod"
gcp_project = "company-prod"
service = "my-app"

[[project.environment.workflow]]
name = "errors-gcp"

[[project.environment.workflow]]
name = "user-activity-gcp"
exclude_users = ["bot@company.com"]

[[project.environment]]
env = "dev"
gcp_project = "company-dev"
service = "my-app"

[[project.environment.workflow]]
name = "errors-gcp"
```

A project may have both `[[project.workflow]]` entries (codebase-level, e.g.
PR review) and `[[project.environment]]` entries (deployment-level, e.g. logs)
at the same time.

### Monitor

`[monitor]` holds non-project observations — home server health, calendar,
anything not tied to a codebase. Same workflow object model:

```toml
[[monitor.workflow]]
name = "home-server-health"
```

### Workflow config

Workflow objects have `name` as the only required field. Everything beyond
that is defined by the individual workflow implementation. The config parser
passes the full object to the workflow; the workflow reads what it needs.
Per-workflow schemas live in `config/schemas/workflows/` and document what each
workflow accepts.

### Schema

`config/schemas/hub.toml.schema.json` is a JSON Schema that validates the
structure of `hub.toml`. Taplo (the standard TOML LSP) uses it to provide
completions, validation, and inline documentation in editors. `.taplo.toml`
at the repo root points taplo at the schema.

### Why always objects, never a string list

A string list (`workflows = ["github-prs"]`) can only carry a name. The
moment any workflow needs per-instance config, the project must be rewritten
to use objects. Allowing both forms means readers must know two syntaxes;
parsers must handle two shapes; the distinction between "simple" and
"configured" workflows blurs over time. Objects everywhere are consistent and
require no migration when config needs to be added.

Each device has its own `hub.toml` listing only the projects relevant to that
machine. When hub-private is in use, per-device configs live in
`hub-private/devices/<name>.toml` and are symlinked to the hub root.

## Consequences

- The `config` crate will parse `hub.toml` and read env vars, delivering a
  single typed `Config` struct downstream. Currently only env var loading is
  implemented; `hub.toml` parsing is next.
- `hub.toml.example` and `.env.example` are committed to the repo and kept
  up to date as workflows are added.
- A project whose required env vars are absent produces no items — clean
  degradation when a service is unconfigured.
- Per-workflow config (e.g. polling cadence) fits naturally as additional
  fields on the workflow object; no schema changes required.
