# config/schemas

JSON Schema files for hub configuration.

## hub.toml.schema.json

Validates `hub.toml` device configuration files. Wired to `taplo check` via `.taplo.toml`.

### Conventions

- Public workflow definitions in `definitions` are sorted alphabetically by workflow name slug (e.g. `errors-gcp` before `github-prs`).
- The `oneOf` array in `definitions.workflow` must stay in the same alphabetical order, with `workflow_private` last — it is a catch-all and must not appear before any specific variant.
- `workflow_private` is a catch-all that matches any workflow name not otherwise recognized — it accepts any `name` string plus arbitrary extra fields. Its `not.enum` list enumerates every known public workflow name. **When adding a new public workflow, add its name to that list** so the catch-all doesn't match it first, which would bypass the specific field constraints defined for that workflow.
