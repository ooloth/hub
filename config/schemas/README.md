# config/schemas

JSON Schema files for hub configuration.

## hub.toml.schema.json

Validates `hub.toml` device configuration files. Wired to `taplo check` via `.taplo.toml`.

### Conventions

- Workflow definitions in `definitions` are sorted alphabetically by workflow name slug (e.g. `errors-gcp` before `github-prs`).
- The `oneOf` array in `definitions.workflow` must stay in the same alphabetical order.
