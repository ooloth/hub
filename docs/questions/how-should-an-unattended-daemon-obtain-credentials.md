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

An unlocked vault is not sufficient either. `op read` reaches `my.1password.com` on every
resolution even when the desktop app's CLI integration is doing the authorising, so a daemon on a
machine with no connectivity fails before it reaches any source. That is a second, independent way
credentials go missing, and it behaves differently from the first: a lock makes `op read` wait for
a prompt, while an unreachable service makes it return an error promptly. A retry policy tuned for
one is wrong for the other.

Both modes fail earlier than the refresh does. `Config::load` resolves every reference before the
first fetch, so neither surfaces as a failed source inside a `StatusReport`, and the invariant that
[a refresh reaching no source never replaces the
cache](../invariants/a-refresh-that-reached-no-source-never-replaces-the-cache.md) does not cover
them, because no refresh happened. Whatever the health record in Phase 3.4 reports has to tell a
locked vault, an unreachable vault and a dead source apart, or a four-second network blip reads as
"credentials unavailable".

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
- Whether each candidate resolves without reaching a remote service, and if it does reach one, what
  the daemon does during an outage. This discriminates between the options in a way the others do
  not: an option resolving locally keeps a pass running through a network blip, and an option
  resolving remotely loses every pass the blip covers, including passes whose sources were fine.

Then the smallest spike that produces an observation: provision the narrowest candidate that clears
those facts, run the daemon across a reboot and across several hours with nobody touching the
machine, and confirm it never prompts and never blocks. Cut connectivity for part of that window,
since an unreachable credential service is the failure mode most likely to occur in ordinary use.
Reading about non-interactive auth does not settle whether this particular daemon stays running.

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
  inside. It also keeps both failure modes rather than one, since it stays network-dependent.
- **B. A scoped non-interactive 1Password credential.** Strongest case: keeps one vendor, one
  provisioning story and the existing `op://` reference shape, so `hub.toml` may not change at all.
  Cost: unknown until the scoping and tier facts are established, and the token has to live
  somewhere that is not itself a plaintext secret on disk. It removes the unlock prompt but is
  expected to keep the network dependency, since a service account authenticates against 1Password
  rather than the local app — *Unverified*, and worth checking early, because it decides whether
  this option fixes one failure mode or both.
- **C. macOS Keychain with an ACL for the daemon binary.** Strongest case: an OS-managed store, no
  second vendor, access bound to a specific binary rather than to anyone who can read a file, and
  resolution stays on the machine, so a pass survives an outage that would stop B. Cost: unknown
  whether non-interactive reads survive reboots and binary rebuilds, and it is macOS-only, which
  forecloses running the daemon anywhere else.
- **D. A provisioned file or launchd environment entry.** Strongest case: simplest possible, depends
  on nothing, works identically everywhere, and resolves locally. Cost: a plaintext secret protected
  only by file permissions, which `~/.agents/standards/security.md` treats as one mistake away from
  failing, and it is the option that most directly contradicts the constraint above.

The network axis does not pick a winner on its own, and it should not be allowed to: it favours C
and D, and D is the option the security constraint most directly rules out. What it does is stop B
being scored as though it removes the whole problem when it may only remove the prompt.

## Findings

Findings are working evidence, not settled fact. Nothing here binds a decision until it graduates
into a record.

- With the 1Password account signed out, `op whoami` reports "account is not signed in" and `op
  read` blocks instead of returning an error. A `hub-tui` process spawned an `op` child and hung
  before reaching the alternate screen, showing nothing, until the account was signed in. *Measured*,
  2026-09-19.
- `op read` reaches `my.1password.com` on every resolution, with the desktop app's CLI integration
  enabled and the vault unlocked. Running `hub-daemon` with egress pointed at a closed port failed
  at config load with `could not read secret 'op://...': error initializing client: Get
  "https://my.1password.com/api/v2/account/keysets?...": proxyconnect tcp: dial tcp 127.0.0.1:1:
  connect: connection refused`. The integration removes the interactive unlock; it does not make
  resolution local. *Measured*, 2026-09-19.
- `op whoami` is not a test of whether credentials are obtainable. With the CLI integration enabled
  it reports "account is not signed in" while `op read` succeeds, because the integration authorises
  individual reads rather than creating a CLI session. Diagnosing availability by running `op
  whoami` reads as a signed-out vault when nothing is wrong. *Measured*, 2026-09-19.
- Two states behave differently and are easy to conflate. With the desktop app **running** and the
  CLI integration enabled, `op read` raises a biometric prompt, the account holder approves it, and
  resolution succeeds. With the app **not running**, `op read` blocks with no prompt to answer and
  the caller hangs. Only the second is the hang described above. *Measured*, 2026-09-21: `just
  daemon` completed `refresh=ok profile=dev items=1136 failed_sources=0` after a fingerprint
  approval, while `op whoami` reported "account is not signed in" both immediately before and
  immediately after that run.
- Approval is per read, not per process. One `Config::load` raises several prompts rather than one,
  because it resolves every `op://` reference in `hub.toml` before the first fetch. *Reported* by
  the account holder, 2026-09-21, who sees "lots of prompts, not one per window"; the exact count
  per run has not been measured.
- Together those sharpen the cost of option A. "The daemon stops when the vault locks" understates
  it: the daemon stops whenever nobody is present to answer a prompt, and a polling daemon would
  raise a burst of them on every pass rather than one. A is therefore not a quiet baseline that
  merely misses notifications — running it attended is itself disruptive.
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
