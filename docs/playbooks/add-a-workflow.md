# Add a Workflow

## Should you add this?

A workflow earns its place if it contributes to at least one of hub's three value layers:
cross-domain urgency ranking, pre-loaded investigation context, or automated proposals.
The test: does this workflow make hub a better starting point than going directly to the
source tool?

A workflow that only mirrors data already visible in GitHub, Grafana, etc. without adding
triage or proposal value doesn't pull its weight. Aggregation alone is not sufficient.

## How to add it

A workflow fetches data from one or more clients and returns a list of items for the UI
to display.

## 1. Add the client (if needed)

If no existing client covers the external API:

1. Create `clients/src/<service>/mod.rs` (or `clients/src/<service>.rs`)
2. Add `pub mod <service>;` to `clients/src/lib.rs`

Client functions should be `async`, accept credentials as `&str` parameters,
and return `anyhow::Result<Vec<YourDomainType>>`.

## 2. Add domain types (if needed)

Add any new structs the workflow operates on to `domain/src/lib.rs`. Keep
them pure — no I/O, no imports from other hub crates.

Each domain type that surfaces items in `hub status` needs two standard fields:
`urgency: domain::Urgency` and `age: chrono::Duration`. These drive the
unified sort order (`urgency` ascending, then `age` descending within a tier).

## 3. Implement the workflow

1. Create `workflows/src/<workflow-name>.rs`
2. Add `pub mod <workflow-name>;` to `workflows/src/lib.rs`

Expose a `pub async fn run(...)` that calls client functions and returns a
typed result. Credentials and config are passed as parameters; the caller
(CLI / TUI) is responsible for loading them.

Assign `urgency` on each item using rules the workflow owns — the workflow is
the right place to encode domain knowledge like "a CI failure is always High"
or "an issue assigned to me is Medium". Use `domain::Urgency::{Critical, High,
Medium, Low}`.

**Error handling:** return `Err` if the upstream API is completely unavailable
(network error, auth failure). The status orchestrator propagates the error and
hub surfaces it to the user. Do not silently return an empty vec when credentials
are missing — that looks identical to "no items", which hides the problem.
If a workflow calls multiple APIs and one fails, propagate the first error
rather than returning partial results; partial data in a unified ranked list
is harder to reason about than a clear error.

## 4. Wire into hub status

Workflows that surface items in `hub status` plug into the unified pipeline —
they don't get their own CLI command.

1. Add one or more variants for your item type(s) to the `StatusItem` enum in
   `workflows/src/status.rs`
2. In `workflows::status::run`, call your new workflow and push its items into
   the shared `Vec<StatusItem>` using those variants
3. Add a match arm for each new variant in `render_line` in
   `ui/cli/src/commands/status.rs` that prints `[tier]  <formatted fields>`

## 5. Register in the Rust config

In `config/src/toml.rs`, add a variant to the `WorkflowConfig` enum:

```rust
#[serde(rename = "my-workflow")]
MyWorkflow {
    // any optional fields with #[serde(default)]
},
```

Add a corresponding case to the `all_workflow_types_parse_with_name_only`
rstest. Unknown workflow names are a hard parse error — this step is
required before any hub command will accept the new name in hub.toml.

## 6. Register in the config schema

In `config/schemas/hub.toml.schema.json`:

1. Add a `"workflow_<name>"` definition under `"definitions"` following the
   same shape as the existing entries (`type`, `description`, `required`,
   `additionalProperties`, `properties` with a `name` const)
2. Add `{ "$ref": "#/definitions/workflow_<name>" }` to the `"workflow"` oneOf
   (keep both lists alphabetical; `workflow_private` stays last)
3. Add the new name to the `not.enum` list inside `workflow_private` — this
   prevents the catch-all from matching the new public workflow name, ensuring
   its specific field constraints apply instead

## 7. Add to the example config

Add an example entry to `hub.toml.example` showing how to enable the workflow
under `[[project.workflow]]`, `[[project.environment.workflow]]`, or
`[[monitor.workflow]]` as appropriate.

## Private workflows

If the workflow is private (not for the public repo), add it to `hub-private`
instead: client in `hub-private/clients/src/`, workflow in
`hub-private/workflows/src/`, and re-export it from the respective `mod.rs`.
See [Add a Private Workflow](add-a-private-workflow.md) and [Private Workflows](../architecture/private-workflows.md) for the full model.

## Done when

`just check` and `just test` pass, and adding the workflow name to `hub.toml`
causes `just cli` (or `just tui`) to display the workflow's items in the
status output.
