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
an interval with nobody present. The daemon is a LaunchAgent, so it runs only inside a logged-in
session and launchd starts it at login, when someone is at the machine. A restart is different:
`KeepAlive` or an install can restart it while nobody is there, and the prompt that restart raises
has no one to answer it. Retrying a prompt nobody will answer is not resilience. Worse than failing:
an unanswered prompt makes `op read` block rather than error, so the caller hangs with nothing on
screen.

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
fix. A daemon that cannot obtain its credentials at startup never runs a pass, and one whose
credential expires while it runs fails every pass after that. Either way no notifications arrive,
and a notification that does not arrive is indistinguishable from nothing having happened. The
failure is silent by construction.

The counter-constraint is what makes this a question rather than a task. Whatever replaces the
interactive unlock is a standing credential on a machine that runs unattended, so it widens what a
compromise of that machine reaches. A model that grants hub's daemon broad access to a personal
vault trades an ergonomic problem for a materially worse security one.

Scoping is the whole difficulty, and it is not small: the daemon needs the full set hub uses, not a
subset. `StatusParams::extra_credentials` in `workflows/src/status.rs` is passed into the private
workflows, so the daemon's fetch pass touches the GitHub token, the Linear token, the Loki token and
every private credential. There is no version of this where the daemon gets by with less than hub's
whole credential set, which means the isolation has to come from what that set is stored beside
rather than from asking for fewer of them.

## What would settle it

The vendor facts and the spikes in Findings leave option A′ below as the only candidate that meets
every requirement. What remains is to confirm it against the real daemon rather than a spike
binary:

- Run `hub-daemon` with one `op inject` call at startup under launchd across a logout and login,
  and confirm the start costs one Touch ID prompt and one macOS dialog naming `hub-daemon`, and
  nothing more for the rest of the session.
- Leave those prompts unanswered past the resolution deadline, and confirm the health record and
  the log say the daemon is waiting for approval, and that a notification says so.
- Start it with the network cut, and confirm it retries inside the process instead of exiting, so
  `KeepAlive` never respawns it into fresh prompts.
- Decide whether two clicks per start is acceptable, or whether the macOS dialog is worth removing.
  A remembered approval for `hub-daemon` would need a stable code identity, and the spikes found
  that neither a rebuild nor even a restart of the same build keeps it.

The items hub needs are three `op://` references, all in vault `Scripts`, on the device the spikes
ran on (read from `hub.toml` 2026-10-12). The other device's configuration has one.

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

## Requirements

Each one comes from a moment the daemon goes through, and each cites its source.

- **R1. Nothing has to be started or remembered.** launchd starts the daemon at login, and no
  command, TUI launch or other manual step stands between login and the first pass. Source: #327's
  ideal state, "without me starting or remembering to open anything".
- **R2. At most one approval per login session, and only one the system brings to the account
  holder.** A prompt that appears and is answered is acceptable. Locking or quitting 1Password
  afterwards does not stop passes. Source: the account holder, 2026-10-12.
- **R3. Nothing the daemon holds leaks.** No secret is stored on disk, and no other process running
  as the same user can read one without the account holder's approval. A Keychain item that any
  same-user process can read through `/usr/bin/security`, which is how `gh` appears to keep its
  token, does not meet this. Source: the account holder, 2026-10-12, and
  `~/.agents/standards/security.md`, under which secrets live only in environment variables or
  secret stores.
- **R4. A start that cannot get credentials says so within a deadline.** The health record, the log
  and a notification name the cause: waiting for approval, 1Password unreachable, or the desktop
  app not running. Source: #339's ideal state.
- **R5. A credential failure never makes the daemon exit.** `KeepAlive` restarts an exited daemon
  once a minute, and every start raises fresh prompts (see Findings). Source: `daemon/README.md`
  and the 2026-10-12 spikes.

The network is not a requirement. Every source except the LAN-only private ones needs the network
anyway, credentials are resolved once per process and held for its life, and an outage at startup
costs only that start under R5's retry.

## Options

- **A′. Fingerprint at startup, hardened.** The daemon resolves every reference in one `op inject`
  call at startup, holds the values in memory as `Secret<String>` for the life of the process, puts
  a deadline on resolution, retries inside the process instead of exiting, and reports the waiting
  state per R4. A start costs one Touch ID prompt and one macOS dialog naming the daemon, both
  shown at login; further `op` calls from the same process raise neither (see Findings). Meets R1
  to R5. Costs: two clicks per start rather than one, and a restart while nobody is at the machine
  waits for their return before its first pass. A notification sent then would not reach anyone
  either. Unverified: how long the 1Password approval lasts within one process, which matters only
  if the daemon re-reads a credential.
- **B. A 1Password service account.** A dedicated vault, read-only access to it alone, and item and
  vault IDs in `hub.toml` so each read costs one request. Fails R3: the `ops_` token is the whole
  credential, 1Password says only that it should not be stored in plaintext, and every place it can
  be stored is either C's Keychain problem or D's plaintext problem. On the Families plan, all
  service accounts in the account share 1,000 requests a day, so a daemon crash-looping at one
  restart a minute would spend the quota in hours.
- **C. A Keychain item readable only by a signed `hub-daemon`.** Fails R2 as measured: with a
  self-signed identity, a rebuild carrying the identical designated requirement was refused (see
  Findings), so every install would need a fresh approval. An Apple Development identity, renewed
  yearly in Xcode, might give the item a stable team ID to match; that is unverified. Also needs
  hardened runtime, or a same-user process can inject code into the trusted binary through
  `DYLD_INSERT_LIBRARIES` in the user-writable LaunchAgent plist (unverified).
- **D. A provisioned file or launchd environment entry.** Fails R3: a plaintext secret protected
  only by file permissions.

Rejected:

- **A remote secret store** (a GitHub Actions secret, AWS Secrets Manager, Doppler and similar).
  GitHub's API returns a secret "without revealing its encrypted value", so only an Actions run can
  read one ([REST API: Actions secrets](https://docs.github.com/en/rest/actions/secrets), read
  2026-10-12). Every other remote store needs a credential stored on the machine to fetch from it,
  which moves the problem to that credential rather than removing it.
- **Handing secrets to a running daemon from a terminal**, through a `hub unlock` command or the
  TUI, over the daemon's socket. Might avoid the macOS dialog, since `just tui` is not known to
  raise it (unverified), but fails R1: someone has to run the command after every login.

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
- With the app **not running** (only the browser extension was), `op read` failed within a second
  with `connecting to desktop app: 1Password CLI couldn't connect to the 1Password desktop app`
  rather than blocking. With the app **open and locked**, `op read` raised a Touch ID prompt, and
  once it was approved `hub-daemon` completed `refresh=ok profile=dev items=1065
  failed_sources=1` in 7.8 seconds. `op` was 2.34.0 both times, the same version as the 2026-09-21
  finding above. *Measured*, 2026-10-02. This contradicts that finding's account of the
  not-running state, and neither state tested here reproduces its hang. What produced the hang,
  and what a locked app does when nobody answers the prompt, are unmeasured.
- Approval is per read, not per process. One `Config::load` raises several prompts rather than one,
  because it resolves every `op://` reference in `hub.toml` before the first fetch. *Reported* by
  the account holder, 2026-09-21, who sees "lots of prompts, not one per window"; the exact count
  per run has not been measured.
- Together those sharpen the cost of the interactive model without A′'s hardening. "The daemon
  stops when the vault locks" understates it: the daemon stops whenever nobody is present to answer
  a prompt, and a polling daemon would raise a burst of them on every pass rather than one. That
  model is therefore not a quiet baseline that merely misses notifications — running it attended is
  itself disruptive.
- The daemon's credential needs are not a subset of the TUI's. `StatusParams::extra_credentials` in
  `workflows/src/status.rs` carries them into the private workflows during a fetch pass, so a daemon
  running that pass needs the same set as the TUI. *Measured*, read from the source 2026-09-19.
- `Config` resolves every credential at startup rather than lazily, per
  `~/.agents/standards/rust.md`, which is why an unavailable vault blocks the process rather than
  the first call that needs a secret. *Measured*, `config/src/resolved.rs`.
- A 1Password service account is scoped per vault, with nothing finer, and cannot be given the
  built-in Personal or Private vault or the default Shared vault. Its permissions default to
  `read_items`, and its vault access "is immutable" once created
  ([get started](https://www.1password.dev/service-accounts/get-started),
  [CLI reference](https://www.1password.dev/cli/reference/management-commands/service-account)).
  On the 1Password, Families and Teams plans a token gets 1,000 reads an hour, and on the 1Password
  and Families plans all service accounts in an account share 1,000 requests a day
  ([rate limits](https://www.1password.dev/service-accounts/rate-limits)). `op read` costs "3
  reads", or 1 when given item and vault IDs
  ([use with 1Password CLI](https://www.1password.dev/service-accounts/use-with-1password-cli)).
  The token, prefixed `ops_`, carries the account unlock key itself
  ([security](https://www.1password.dev/service-accounts/security)), and the only storage guidance
  is "don't store it in plaintext". All read 2026-10-12. *Unverified*: whether a service-account
  `op read` works with no network, and whether setting `OP_SERVICE_ACCOUNT_TOKEN` keeps `op` from
  contacting the desktop app; only third-party sources address the second.
- The data protection keychain needs entitlements "authorized by a provisioning profile" and an
  app-like bundle, which is "not for command-line tools", so an ad-hoc `hub-daemon` cannot use it
  ([TN3137](https://developer.apple.com/documentation/technotes/tn3137-on-mac-keychains), read
  2026-10-12). `man codesign` says ad-hoc signing "identifies exactly one instance of code".
- A Keychain item created by a binary signed with a self-signed code-signing certificate, with a
  pinned identifier and hardened runtime, was read by that build in 8 ms with no prompt. A rebuild
  with different code, signed with the same certificate and the same identifier, and so carrying
  the identical designated requirement, was refused with `-25293` (`errSecAuthFailed`) in 11 ms
  while user interaction was disabled. The original build could still read it. Setting up the
  certificate also cost one login-keychain password dialog for `codesign` to use the new key.
  *Measured*, 2026-10-12, macOS 26.6.2, `security-framework` 3.7.0. Why the rebuild was refused is
  inferred, not observed: the item's partition list probably names the first build's code hash,
  because a self-signed certificate carries no team ID.
- With user interaction disabled through `SecKeychain::disable_user_interaction`, a read that would
  prompt fails fast with `-25293` instead. *Measured*, same run.
- Credentials are resolved once per daemon process, not once per pass. `hub-daemon` calls
  `Config::load` once in `daemon/src/main.rs`, before either the `--once` pass or the
  `schedule::every` loop, and every pass reuses that `Config`, so no pass calls `op read`. This
  supersedes the 2026-09-21 reading above that a polling daemon raises a burst of prompts on every
  pass, and the unattended daemon's unlock problem moves from each pass to each start. Prompts, and
  network access to 1Password, matter at startup, restart and reboot. The open risks are therefore
  two: a daemon started while nobody is present (a launch at login or boot), and a credential that
  expires or is revoked while the daemon runs, which no pass can refresh because none re-reads it.
  The 60-second `SOURCE_TIMEOUT` in `workflows/src/status.rs` bounds each source in a pass, not
  credential resolution at startup. *Measured*, read from the source 2026-10-03; not observed by
  running a daemon across a restart.
- `op read` through the desktop app's CLI integration works from a launchd agent with no TTY,
  started in `/` with launchd's PATH, once someone approves the prompt. A throwaway LaunchAgent
  running one `op read` against `op://Scripts/GitHub/personal-access-token-hub` exited 0 with the
  value, after 41.9 seconds waiting on a Touch ID approval. *Measured*, 2026-10-10, macOS 26.6.2,
  ad-hoc signed Rust binary, parent `/sbin/launchd`.
- An approval does not carry over to the next process. The same agent was restarted twice within
  two minutes, each run making one `op read`, and the account holder saw several prompts across
  the three runs rather than one. The later runs finished in 4.9 and 3.6 seconds, which fits a
  quick approval. So every daemon restart, including one `KeepAlive` performs while nobody is
  present, waits on prompts of its own. *Reported* by the account holder, 2026-10-10; the prompt
  count per run was not recorded. 1Password documents authorization as "confined to a terminal
  session" on macOS, and how it scopes a process with no terminal is not documented
  ([app integration security](https://www.1password.dev/cli/app-integration-security/), read
  2026-10-10).
- With nobody there to answer the prompt, `op read` waits rather than failing. A daemon that
  `just install` restarted under launchd printed its startup line at 06:39:11Z, and its first pass
  came at 13:33:33Z, after the account holder returned and approved. Five minutes in, `ps` showed
  the daemon's `op read` child alive and waiting, and the log held only the startup line until that
  first pass. So a restart while nobody is there blocks until someone answers, 6 h 54 min here,
  with no error to retry on. *Measured*, 2026-10-10, macOS 26.6.2, `op` 2.40.0, from the daemon's
  log and `ps`.
- One 1Password approval covers every `op` process started by the same parent process, and no more.
  Under one launchd job, one `op inject` resolving all three references raised one Touch ID prompt,
  and three further `op` processes started by the same shell raised none. Each new launchd job
  raised its own. *Measured*, 2026-10-12, macOS 26.6.2, `op` 2.40.0; prompts reported by the
  account holder through a dialog shown after each step.
- macOS shows "op would like to access data from other apps" for `op` processes started under
  launchd, and the dialog names the launchd job's own binary when that binary starts `op`.
  Answering Allow lasts for that process only. A Rust binary started by launchd ran `op read`
  twice: the first raised Touch ID and the dialog naming the binary, and the second raised
  nothing. A new job running the same build raised both again, and so did a rebuild. With a shell
  as the parent, the dialog named `op` and came back for every `op` process. *Measured*,
  2026-10-12, macOS 26.6.2, `op` 2.40.0, ad-hoc linker-signed binary.
- Setting `OP_LOAD_DESKTOP_APP_SETTINGS=false` does not remove that dialog, and stops the CLI
  integration from finding the account ("No accounts configured for use with 1Password CLI").
  *Measured*, same session.
