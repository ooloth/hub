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

## Keep I/O at the edges

Workflows are intentionally thin: fetch data from clients, pass it to pure
functions, return. Don't put logic in them — put it in `domain/`.

This is the functional core / imperative shell pattern. The shell (clients,
workflows) does I/O and is hard to test without real infrastructure. The core
(domain) is pure logic with no I/O and is easy to test exhaustively.

**Where to put tests, by layer:**

- `clients/` — unit-test parsing and URL construction directly (e.g.
  `repo_slug_from_url`). Don't test network calls.
- `domain/` — unit-test all logic here. This is where correctness lives.
  Parameterize, snapshot, and property-test freely.
- `workflows/` — accept that these require a real API to run end-to-end.
  Cover them with one-off smoke tests, not unit tests.

When you feel the urge to mock a client to test a workflow, treat it as a signal:
there is logic in the workflow that belongs in `domain/` instead. Move it there
and test it directly.

For the workflow layer, the practical substitute for unit tests is a scheduled
smoke run — a cron job or CI schedule that runs `just status` daily or weekly
and alerts on failure. This catches API drift (renamed fields, auth changes,
deprecated endpoints) before you discover it when you actually need hub to work.
`main() -> Result<()>` already exits non-zero on any error, so the exit code is
the signal.
