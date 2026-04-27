# Add a Workflow

Steps to add a new workflow to hub. A workflow fetches data from one or more
clients and returns a list of items for the UI to display.

## 1. Add the client (if needed)

If no existing client covers the external API:

1. Create `clients/src/<service>/mod.rs` (or `clients/src/<service>.rs`)
2. Add `pub mod <service>;` to `clients/src/lib.rs`

Client functions should be `async`, accept credentials as `&str` parameters,
and return `anyhow::Result<Vec<YourDomainType>>`.

## 2. Add domain types (if needed)

Add any new structs the workflow operates on to `domain/src/lib.rs`. Keep
them pure — no I/O, no imports from other hub crates.

## 3. Implement the workflow

1. Create `workflows/src/<workflow-name>.rs`
2. Add `pub mod <workflow-name>;` to `workflows/src/lib.rs`

Expose a `pub async fn run(...)` that calls client functions and returns a
typed result. Credentials and config are passed as parameters; the caller
(CLI / TUI) is responsible for loading them.

## 4. Wire into the CLI

In `ui/cli/src/main.rs`:

1. Add a variant to the `Commands` enum
2. Add a match arm that loads config, calls `workflows::<name>::run(...)`, and
   prints the result

## 5. Register in the config schema

In `config/schemas/hub.toml.schema.json`:

1. Add a `"workflow_<name>"` definition under `"definitions"` following the
   same shape as the existing entries (`type`, `description`, `required`,
   `additionalProperties`, `properties` with a `name` const)
2. Add `{ "$ref": "#/definitions/workflow_<name>" }` to the `"workflow"` oneOf

## 6. Add to the example config

Add an example entry to `hub.toml.example` showing how to enable the workflow
under `[[project.workflow]]`, `[[project.environment.workflow]]`, or
`[[monitor.workflow]]` as appropriate.

## Private workflows

If the workflow is private (not for the public repo), add it to `hub-private`
instead: client in `hub-private/clients/src/`, workflow in
`hub-private/workflows/src/`, and re-export it from the respective `mod.rs`.
See `docs/private-integrations.md` for the full model.
