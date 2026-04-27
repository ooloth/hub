# Testing conventions

## Unit tests

Write inline unit tests (`#[cfg(test)] mod tests`) for pure logic: parsing, validation,
transformations. These live next to the code they cover. Keep assertions focused — test
one behaviour per test function.

## Parameterized tests (`rstest`)

Reach for `rstest` when the same test shape applies to multiple inputs or variants. The
canonical use case is "all workflow types accept a name-only configuration" — one test
function, one `#[case]` per variant, rather than eight copy-pasted functions.

Add `rstest` as a `[dev-dependency]` in the relevant crate's `Cargo.toml`.

```rust
#[rstest]
#[case("errors-gcp", WorkflowConfig::ErrorsGcp { exclude_users: vec![] })]
#[case("github-prs",  WorkflowConfig::GithubPrs { exclude_authors: vec![] })]
fn parses_with_name_only(#[case] name: &str, #[case] expected: WorkflowConfig) { ... }
```

## Snapshot tests (`insta`)

Reach for `insta` when the value under test is large or deeply structured and hand-writing
the expected value would be tedious and fragile. Good targets: the fully-parsed output of
a realistic device config, a rendered TUI frame, a serialized API response.

Add `insta` as a `[dev-dependency]` in the relevant crate's `Cargo.toml`.

```rust
insta::assert_debug_snapshot!(result);
```

To accept or update snapshots after intentional changes:

```bash
just test-update   # INSTA_UPDATE=always cargo nextest run
```

Snapshot files live in `src/snapshots/` next to their test module. Commit them alongside
the test.

## Mutation testing (`cargo-mutants`)

Run `cargo-mutants` to verify that your test suite actually catches logic errors — it
mutates operators, return values, and boolean conditions one at a time and reports which
survived undetected.

```bash
just mutants
```

Run occasionally (before merging a significant logic change, or when a module feels
under-tested), not on every commit. It is intentionally slow.

`cargo-mutants` is not wired into prek or CI because a full run can take minutes.
