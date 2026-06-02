# 003 — Config model: hub.toml for structure and credentials

## Context

Hub runs on multiple devices with different projects active on each (work
laptop vs. personal laptop). As workflows multiply, per-project configuration
is needed: which repo to watch, which issue tracker to query, which log
labels to filter on. Environment variables handle credentials well but are a
poor fit for structured lists of projects and their properties.

## Decision

One gitignored config file with a committed example:

**`hub.toml`** — structure and credentials. Has three top-level concepts:
`[credentials]` for secrets, `[[project]]` for codebases, and `[monitor]`
for non-project observations. Credential values are either plain strings or
1Password references (`op://Vault/Item/field`), resolved at startup via
`op read`. No separate `.env` file or `op run` wrapper is needed.

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
Per-workflow object shapes live as `workflow_<name>` definitions in
`config/schemas/hub.toml.schema.json` and document what each workflow accepts.

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

- The `config` crate parses `hub.toml` and resolves credentials, delivering
  a single typed `Config` struct downstream. Secrets are wrapped in
  `Secret<String>` and exposed only at client call sites.
- `hub.toml.example` is committed to the repo and kept up to date as
  credentials and workflows are added.
- Missing required credentials (`github_token`, `github_username`) produce
  a clear startup error. Optional credentials produce no items when absent.
- Per-workflow config (e.g. polling cadence) fits naturally as additional
  fields on the workflow object; no schema changes required.
