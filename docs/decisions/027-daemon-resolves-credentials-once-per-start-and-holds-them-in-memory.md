---
number: 027
status: accepted
date: 2026-10-12
---

# 027 — The daemon resolves its credentials through 1Password once per start and holds them only in memory

_Status: accepted; not yet implemented. `serve` in `daemon/src/main.rs` still calls
`Config::load` with no deadline and exits when it fails._

## Forced by

- [#327](https://github.com/ooloth/hub/issues/327)'s ideal state: the daemon runs "without me
  starting or remembering to open anything".
- [#339](https://github.com/ooloth/hub/issues/339)'s ideal state: a daemon that cannot obtain
  credentials says so in its log and health record instead of blocking, a reader can tell a locked
  vault, an unreachable credential service and a failing source apart, and running unattended does
  not widen what a compromise of the machine can reach.
- The account holder's requirements, stated 2026-10-12: at most one approval per login session,
  brought to them rather than started by them, and nothing the daemon holds readable by another
  process running as the same user without their approval. They are R1 to R5 in
  [the credentials question](https://github.com/ooloth/hub/blob/c49b3f6/docs/questions/how-should-an-unattended-daemon-obtain-credentials.md#requirements),
  which also holds every measurement cited below.
- Measured under launchd on macOS 26.6.2 with `op` 2.40.0: one Touch ID approval and one macOS
  "access data from other apps" dialog cover every `op` process the daemon starts, and a new
  process needs both again. An unanswered prompt makes `op read` wait rather than fail, for
  6 h 54 min in one observed case.
- `KeepAlive` restarts an exited daemon at most once a minute (`daemon/README.md`).
- [021](021-daemon-owns-signal-refresh.md): the daemon does all the refreshing, so it needs every
  credential a refresh uses.

## Decision

The daemon resolves every credential in `hub.toml` through the 1Password desktop app's CLI
integration once per process, at startup, and holds the values as `Secret<String>` in memory for
the life of the process. No credential is written to disk, to the Keychain or to the environment.
launchd starts the daemon at login, so a start puts one Touch ID prompt and one macOS dialog naming
`hub-daemon` in front of the account holder, and the rest of the session costs nothing more.

Three properties belong to this decision, because without them it fails the requirements:

- Resolution has a deadline. Once it passes, the daemon records that it is waiting for approval in
  the health record ([026](026-daemon-health-is-one-row-in-its-own-table.md)) and the log, sends a
  notification saying so, and keeps waiting.
- A credential failure never makes the daemon exit. It retries inside the process, so `KeepAlive`
  never respawns it into fresh prompts, and a 1Password outage costs a few attempts.
- The health record tells apart waiting for approval, 1Password unreachable and the desktop app not
  running.

The deadline's length, the retry schedule and the notification's wording belong to #339.

Of the four resources, only the network binds. Resolution reaches `my.1password.com`, which is
accepted because every source except the LAN-only private ones needs the network anyway, and
resolution happens once per process rather than once per pass. Memory holds a few short strings.
CPU does not bind, and nothing is stored.

## Rejected

- **A 1Password service account**, because its `ops_` token is the whole credential and has to be
  stored on the machine, which reduces it to the Keychain or plaintext options below. Its strongest
  case is that no prompt is ever needed. Reverses if 1Password offers a token bound to a code
  identity or to the desktop app's approval.
- **A Keychain item readable only by a signed `hub-daemon`**, because with a self-signed identity a
  rebuild carrying the identical designated requirement was refused, so every install would need a
  fresh approval. An Apple Development identity might avoid that, which is unverified and costs a
  yearly renewal per machine. Its strongest case is that a crash restart while nobody is at the
  machine carries on without them. Reverses if restarts while nobody is there become routine and a
  signing identity is shown to keep access across rebuilds.
- **A plaintext file or launchd environment entry**, because any process running as the same user
  can read it. Reverses only if the requirement that nothing held can leak is dropped.
- **A remote secret store** such as a GitHub Actions secret, because fetching from it needs a
  credential stored on the machine, which moves the problem rather than removing it. A GitHub
  Actions secret cannot be read outside an Actions run at all.
- **Handing credentials to a running daemon from a terminal**, because someone has to run the
  command after every login, which is the manual start #327 exists to remove.
- **Not yet: today's startup unchanged**, because `Config::load` then waits on an unanswered prompt
  with nothing recorded, or exits on a fast failure and is restarted into fresh prompts once a
  minute.

## Risk

A restart while nobody is at the machine runs no pass until they return and answer two dialogs. A
notification sent meanwhile would reach no one, but the cache goes stale and the TUI shows it stale.

Each start costs two dialogs rather than one. The account holder accepted that to begin with.

A credential revoked or rotated while the daemon runs fails every pass until the next start,
because nothing re-reads it.

Credentials sit in the daemon's memory. Root, or a process allowed to debug the daemon, can read
them, and core dumps are not addressed here.

How 1Password approves an `op` process with no terminal is undocumented and was measured on one
`op` and macOS version. An update to either can change what a start costs.

## Revisit when

The health record shows the daemon waiting for approval at the account holder's return more than
occasionally. Or an `op` or macOS update makes a start cost more than one approval of each kind.
Or the login dialogs prove a nuisance worth the signing route's maintenance.

## Also update

- [x] questions/README.md — nothing to change. This settles the credentials question, which is
      deleted; its findings are linked above at `c49b3f6`
- [x] vision.md — nothing defined or foreclosed; it does not mention credentials
- [ ] architecture/secrets.md — describes what is built, so it changes in the change that builds
      this, under #339
