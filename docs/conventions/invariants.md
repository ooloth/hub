# Invariants and assertions

Hub follows the [Tiger Style](https://tigerstyle.dev/) principle: if something must be true, assert it. Assertions
are executable documentation — they make assumptions visible to the next reader and crash
loudly instead of silently producing wrong output.

From the doc:

> Where types check structure, assertions check all logic and state, to detect programmer error,
> multiply fuzzing, and downgrade catastrophe. Assert arguments, returns, and invariants: what you
> expect and don't expect, the positive and negative space, not only contract but breach.

## The rule

If a condition must hold given correct code, encode it as an `assert!`. Don't leave it
as a comment, don't silently swallow the violation, don't paper over it with a fallback.
Assert it.

```rust
fn most_urgent(items: &[WorkItem]) -> &WorkItem {
    assert!(!items.is_empty(), "caller must guarantee at least one item");
    items.iter().max_by_key(|i| i.priority).unwrap()
}
```

```rust
fn mark_resolved(issue: &mut Issue) {
    assert_eq!(issue.state, IssueState::Open, "only open issues can be resolved");
    issue.state = IssueState::Resolved;
}
```

```rust
fn summarise(fetched: &[Item], reported: usize) {
    assert_eq!(fetched.len(), reported, "fetch count must match report count");
}
```

`assert!` fires in both debug and release builds. That's intentional. A violated
invariant means the program is in an unexpected state — it is safer to crash than to
continue and corrupt data or silently return wrong results.

## assert! vs Result

`assert!` is for programmer errors: conditions that must hold given correct code and
correct inputs upstream. Use `Result` at system boundaries — absent env vars, malformed
files, failed network calls, and unexpected external API shapes are environmental
conditions the caller should handle, not bugs to assert away.

The test: _could this fire due to something outside my code?_ If yes, use `Result`. If
only a bug within the codebase could cause it, use `assert!`.

## debug_assert!

Use `debug_assert!` only when a check is genuinely expensive on a hot path and the
invariant is enforced structurally elsewhere in release builds. In practice, `assert!`
is almost always the right choice — the overhead is negligible and the safety guarantee
holds in production where it matters most.
