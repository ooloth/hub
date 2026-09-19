---
opened: 2026-09-19
status: open
resolves_into: decision
---

# How should an unattended daemon obtain credentials?

## Why it matters

Hub's credential model assumes a person is at the keyboard. `op://` references in `hub.toml` are
resolved at startup by shelling out to `op read`, which goes through the 1Password desktop app and
gates on an interactive unlock. That is fine for a TUI somebody just launched.

[Decision 020](../decisions/020-hub-runs-an-unattended-surface.md) and
[021](../decisions/021-daemon-owns-signal-refresh.md) put a daemon behind that same model, waking on
an interval with nobody present. A fingerprint prompt has no one to answer it, and retrying a prompt
nobody will answer is not resilience. Worse than failing: with the account signed out, `op read`
blocks rather than erroring, so the caller hangs with nothing on screen.

The consequence lands exactly on what [#327](https://github.com/ooloth/hub/issues/327) exists to
fix. A daemon that stops when the vault locks stops sending notifications, and a notification that
does not arrive is indistinguishable from nothing having happened. The failure is silent by
construction.

The counter-constraint is what makes this a question rather than a task. Whatever replaces the
interactive unlock is a standing credential on a machine that runs unattended, so it widens what a
compromise of that machine reaches. A model that grants hub's daemon broad access to a personal
vault trades an ergonomic problem for a materially worse security one.

Scoping is the whole difficulty, and it is not small: the daemon needs the full set hub uses, not a
subset. `workflows/src/status.rs:206` passes `extra_credentials` into the private workflows, so the
daemon's fetch pass touches the GitHub token, the Linear token, the Loki token and every private
credential. There is no version of this where the daemon gets by with less than hub's whole
credential set, which means the isolation has to come from what that set is stored beside rather
than from asking for fewer of them.

## What would settle it

Vendor facts first, established rather than recalled, because every option below turns on details
nobody here has checked:

- What a 1Password service account token can be scoped to, whether per-vault scoping is fine enough
  to expose hub's items and nothing else, what account tier it requires, and where the token then
  lives so that it is not itself the unprotected secret.
- Whether a launchd agent can read a macOS Keychain item non-interactively, and whether that
  survives a reboot, an OS update, and a rebuild of the binary that owns the ACL.

Then the smallest spike that produces an observation: provision the narrowest candidate that clears
those facts, run the daemon across a reboot and across several hours with nobody touching the
machine, and confirm it never prompts and never blocks. Reading about non-interactive auth does not
settle whether this particular daemon stays running.

One input is already available and should be gathered while filing: the exact list of items hub
needs, since "scope it to hub's credentials" is only actionable against a list.

## Resolves into

[../decisions/](../decisions/), as a record on how secrets reach an unattended process. It moves a
boundary: `../architecture/secrets.md` documents the current model as 1Password to `op read` to
`Secret<String>`, and changing it is a provisioning change for every credential rather than an edit.
That document is updated by the same change.

## Source

Raised 2026-09-19 while filing [#339](https://github.com/ooloth/hub/issues/339), after the TUI hung
on a signed-out 1Password account during unrelated work. The reasoning was recorded in that issue
first and promoted here, since an issue comment is invisible to a listing of this folder and
disappears from view when the issue closes.

Related from the other direction: [#94](https://github.com/ooloth/hub/issues/94) asks for a local
run path that does not require 1Password, which is the same tension for a developer rather than a
daemon.

## Options

- **A. Keep the interactive model and accept that the daemon stops when the vault locks.** Strongest
  case: no new credential exists anywhere, so nothing widens. It is also the honest baseline, and
  every other option has to beat it rather than merely differ from it. Cost: notifications stop
  silently, which is the failure #327 exists to remove, so this option defeats the milestone it sits
  inside.
- **B. A scoped non-interactive 1Password credential.** Strongest case: keeps one vendor, one
  provisioning story and the existing `op://` reference shape, so `hub.toml` may not change at all.
  Cost: unknown until the scoping and tier facts are established, and the token has to live
  somewhere that is not itself a plaintext secret on disk.
- **C. macOS Keychain with an ACL for the daemon binary.** Strongest case: an OS-managed store, no
  second vendor, and access bound to a specific binary rather than to anyone who can read a file.
  Cost: unknown whether non-interactive reads survive reboots and binary rebuilds, and it is
  macOS-only, which forecloses running the daemon anywhere else.
- **D. A provisioned file or launchd environment entry.** Strongest case: simplest possible, depends
  on nothing, and works identically everywhere. Cost: a plaintext secret protected only by file
  permissions, which `~/.agents/standards/security.md` treats as one mistake away from failing, and
  it is the option that most directly contradicts the constraint above.

## Findings

Findings are working evidence, not settled fact. Nothing here binds a decision until it graduates
into a record.

- With the 1Password account signed out, `op whoami` reports "account is not signed in" and `op
  read` blocks instead of returning an error. A `hub-tui` process spawned an `op` child and hung
  before reaching the alternate screen, showing nothing, until the account was signed in. *Measured*,
  2026-09-19.
- The daemon's credential needs are not a subset of the TUI's. `workflows/src/status.rs:127` and
  `:206` carry `extra_credentials` into the private workflows during a fetch pass, so a daemon
  running that pass needs the same set as the TUI. *Measured*, read from the source 2026-09-19.
- `Config` resolves every credential at startup rather than lazily, per
  `~/.agents/standards/rust.md`, which is why an unavailable vault blocks the process rather than
  the first call that needs a secret. *Measured*, `config/src/resolved.rs`.
- 1Password offers service accounts intended for unattended use. *Unverified*: nobody has checked
  what they can be scoped to, what tier they need, or how the token is meant to be stored.
- A launchd agent can hold a Keychain ACL permitting non-interactive reads. *Unverified*: stated
  from general recollection, with nothing checked and no attempt made.
