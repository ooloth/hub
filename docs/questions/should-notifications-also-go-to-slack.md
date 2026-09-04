---
opened: 2026-09-04
status: open
resolves_into: decision
---

# Should notifications also go to Slack, alongside the local OS notification?

## Why it matters

A local macOS notification is the v1 delivery channel — no OAuth, no external service, no risk of
a message landing in the wrong channel. Slack would add multi-device visibility (seeing the digest
on a phone, not just the laptop that's running the daemon) and richer formatting for a multi-item
digest. It isn't needed for the milestone to succeed, but the design shouldn't make adding it later
expensive.

## What would settle it

Actually living with local-only notifications for a while and finding a concrete case where their
single-device reach is the reason something got missed.

## Resolves into

`../decisions/` — a new ADR, if and when this is picked up.

## Source

Raised during the PR-notification-and-auto-launch design discussion (in `ooloth/dotfiles`,
2026-09-04), migrated here once the decision to build in hub was made. See the epic issue for full
context.

## Options

- **A. Local OS notification only.** Current plan. Simplest, zero external dependency, zero
  channel-mistargeting risk.
- **B. Slack DM, in addition to or instead of local.** Needs a verified send capability
  (unauthenticated in the originating session as of 2026-09-04) and a design that structurally
  cannot target anything but the user's own DM — never a channel, given the stated concern about
  ever posting somewhere public or wrong.
- **C. Both**, local as the reliable baseline and Slack as an optional richer mirror.

## Findings

_Findings are working evidence, not settled fact. Nothing here binds a decision until it
graduates into a decision record._

...
