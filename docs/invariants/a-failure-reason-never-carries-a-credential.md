# A failure reason never carries a credential

## The invariant

No credential that a refresh is given, and no part of one that is a URL, appears in the reason
recorded for a source that failed.

## Why it must hold

A reason is written where nothing treats it as secret. The daemon prints it in its one line per
pass, which lands in a log file, and the health record in
[#341](https://github.com/ooloth/hub/issues/341) stores it in the profile's database. Both are read
with `cat` and `sqlite3`, copied into issues, and kept.

Clients put credentials into their errors without meaning to. reqwest's own error text includes the
request URL, and a client that adds `failed to reach {endpoint}` as context repeats it. When the
service URL is itself a credential, or carries one in its username, password or query string, the
first network failure would write it out, and every pass while the service is down would write it
again.

## What it forbids

- Formatting a source's error for the log or the database anywhere except through
  `SourceFailure::new`, which is the only way to make a `FailureReason`.
- A source that builds its own reason string, or hands back text instead of its error. The private
  workflows push a `SourceError` holding the error itself, so their failures are redacted like
  hub's own.
- Adding a credential to `StatusParams` without adding it to `known_secrets` in
  `workflows/src/status.rs`, which is what tells redaction it exists.
- Cutting a reason to length before redacting it, which can leave the start of a credential behind
  where the cut fell.

## How it is enforced

- `FailureReason` has a private field and one constructor, `SourceFailure::new`, which redacts every
  value in `KnownSecrets` before anything else touches the text.
  (`workflows/src/source_failure.rs`, `workflows/src/known_secrets.rs`)
- That constructor asserts its result holds no known secret, and halts the pass if it does.
- `no_known_secret_survives_into_a_reason` is a property test that hides generated secrets
  throughout a multi-line error chain, including one secret inside another and secrets made of
  regex metacharacters, and checks none survives.
- `every_credential_a_refresh_is_given_is_redacted_from_failure_reasons` checks that each kind of
  credential in `StatusParams` is known to redaction.
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
