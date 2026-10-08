# A failure reason never carries a credential

## The invariant

No credential that a refresh is given, and no part of one that is a URL, appears in the reason
recorded for a source that failed, or in the reason recorded for a daemon pass that failed as a
whole.

## Why it must hold

A reason is written where nothing treats it as secret. The daemon prints it in its one line per
pass, which lands in a log file, and stores it in the profile's database as part of its health
record ([Decision 026](../decisions/026-daemon-health-is-one-row-in-its-own-table.md)). Both are
read with `cat` and `sqlite3`, copied into issues, and kept.

Clients put credentials into their errors without meaning to. reqwest's own error text includes the
request URL, and a client that adds `failed to reach {endpoint}` as context repeats it. When the
service URL is itself a credential, or carries one in its username, password or query string, the
first network failure would write it out, and every pass while the service is down would write it
again. A pass that fails as a whole carries whatever error stopped it, and nothing guarantees that
error is free of the same text.

## What it forbids

- Formatting a source's error or a pass's error for the log or the database anywhere except through
  `SourceFailure::new` or `PassFailure::new`, which are the only ways to make a `FailureReason`.
- A source that builds its own reason string, or hands back text instead of its error. The private
  workflows push a `SourceError` holding the error itself, so their failures are redacted like
  hub's own.
- Adding a credential to `StatusParams` without adding it to `known_secrets` in
  `workflows/src/status.rs`, which is what tells redaction it exists. The daemon builds its
  `KnownSecrets` through the same function, before the refresh consumes the parameters.
- Cutting a reason to length before redacting it, which can leave the start of a credential behind
  where the cut fell.

## How it is enforced

- `FailureReason` has a private field and two constructors, `SourceFailure::new` and
  `PassFailure::new`, both of which redact every value in `KnownSecrets` before anything else
  touches the text. (`domain/src/source_failure.rs`, `domain/src/pass_failure.rs`,
  `domain/src/known_secrets.rs`)
- The types live in `domain/`, so `store::daemon_health::record` takes them directly. The health
  record stores no failure text that did not come through one of those constructors, which the
  compiler checks.
- The shared constructor asserts its result holds no known secret, and halts the pass if it does.
- `no_known_secret_survives_into_a_reason` and `no_known_secret_survives_into_a_pass_failure` are
  property tests that hide generated secrets throughout a multi-line error chain, including one
  secret inside another and secrets made of regex metacharacters, and check none survives.
- `every_credential_a_refresh_is_given_is_redacted_from_failure_reasons` checks that each kind of
  credential in `StatusParams` is known to redaction, and
  `a_credential_the_refresh_was_given_is_redacted_from_a_pass_failure` in `daemon/src/cache.rs`
  checks that the daemon redacts with the same credentials.
- `a_secret_where_a_link_is_cut_leaves_none_of_itself_behind` checks that redaction happens before
  the cut.

What these do not cover:

- A credential a client transforms before writing it into an error, for example by encoding it.
  Redaction matches the text of each value and each URL part, nothing derived from them.
- A credential a source holds that never passes through `StatusParams`, since redaction cannot know
  it exists.
- A credential that contains `…` or `: `. Cutting a reason inserts `…`, and joining the links of an
  error chain inserts `: `, so either could complete such a credential after redaction ran. The
  assertion would then halt the pass rather than write it.
- An error raised before the daemon has its credentials, such as `Config::load` failing at startup.
  Nothing records it in the database yet; Phase 3.6 of
  [#327](https://github.com/ooloth/hub/issues/327) does.
