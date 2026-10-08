# An unconfigured source is never asked

## The invariant

A refresh asks a source only when `hub.toml` configures it, so a source with no configuration is
named neither among the sources that answered nor among those that failed.

## Why it must hold

The daemon's health record tells a pass where every source failed from one where some answered by
whether any source answered ([Decision 026](../decisions/026-daemon-health-is-one-row-in-its-own-table.md)).
A source asked without its configuration has nothing to fetch, so it answers with nothing, and that
empty answer counts. On a device without that source configured, an outage of every real source
would then be recorded as `partial`, and `no_source_answered` could never occur there.

## What it forbids

- A source that is always asked and returns an empty answer when its credential or environment is
  missing. This is the tempting form, because it reads as harmless: the empty list is accurate.
- Reporting an unconfigured source as failed. Nothing went wrong, and a reader would see a failure
  on every pass of a device that simply does not use the source.

## How it is enforced

- `sources_are_named_as_the_report_has_always_named_them` in `workflows/src/status.rs` checks that
  a refresh with no Linear token does not ask Linear, and
  `linear_is_asked_only_when_a_linear_token_is_configured` checks that one with a token does.
- Loki and GCP are asked once per configured environment, so an unconfigured one has nothing to ask.

What these do not cover: a new source added to `sources` in `workflows/src/status.rs` that is
asked whether or not it is configured. No check enumerates the sources against their
configuration, because whether a source is configured is a different test for each one.
