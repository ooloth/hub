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

## Property-based tests (`proptest`)

Reach for `proptest` when you can state an invariant that should hold for any valid input,
not just the handful of examples you thought to write. Good targets: round-trip properties
("any valid name survives parse → serialize → parse unchanged"), structural invariants
("N projects in always gives N projects out"), and boundary conditions you would not
enumerate by hand.

Add `proptest` as a `[dev-dependency]` in the relevant crate's `Cargo.toml`.

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn project_fields_round_trip(
        name in "[a-zA-Z][a-zA-Z0-9_. -]{0,30}",
        owner in "[a-zA-Z][a-zA-Z0-9_-]{0,15}",
        repo  in "[a-zA-Z][a-zA-Z0-9_-]{0,15}",
    ) {
        let toml = format!("[[project]]\nname = \"{name}\"\nrepo = \"{owner}/{repo}\"\n");
        let result = parse(&toml).unwrap();
        prop_assert_eq!(&result.project[0].name, &name);
        prop_assert_eq!(&result.project[0].repo, &format!("{owner}/{repo}"));
    }
}
```

Use regex strategies (`"[a-z][a-z0-9-]{0,15}"`) to constrain generated strings to valid
inputs when the function under test has preconditions (e.g. valid TOML, no embedded
quotes). Use `proptest::collection::vec(strategy, range)` to generate variable-length
lists.

When proptest finds a failing case it writes a minimal reproduction to
`proptest-regressions/` (gitignored). Re-running the test will replay the saved case
before generating new ones.

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
