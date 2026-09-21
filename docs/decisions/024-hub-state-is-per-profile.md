---
number: 024
status: accepted
date: 2026-09-20
---

# 024 — Hub state is per-profile under `~/.hub/<profile>`

_Status: accepted; not yet implemented. `store::status_cache::db_path` resolves
`~/.hub/hub.db` with no profile in it (`store/src/status_cache.rs:33-37`)._

## Forced by

[Decision 020](020-hub-runs-an-unattended-surface.md) gives hub a long-lived process, and Phase 3.6
of [#327](https://github.com/ooloth/hub/issues/327) puts it under launchd. From that point a daemon
writes the cache on an interval with nobody present, while development runs the same binaries from
source against the same files. `store::status_cache::db_path` resolves one path for both
(`store/src/status_cache.rs:33-37`).

What the two share, measured on this machine 2026-09-20:

- `~/.hub/hub.db`, 5.3 MB, rewritten by every refresh. This is the object that collides.
- `~/.hub/repos/`, 2.0 GB across 19 projects: bare clones and their per-branch worktrees.

`just qa-seed` already works around the sharing. It copies the database aside, refuses to run
twice, and needs `qa-restore` afterwards including when a run fails partway
(`scripts/seed-signal.py`). A daemon polling on an interval overwrites a seeded row before a QA run
finishes, so that workaround stops working at Phase 3.3 rather than degrading.

## Decision

Hub's own state is per-profile. `HUB_PROFILE` names one directory under `~/.hub/`, and the
database, along with the lock, socket and log when those are built, lives at `~/.hub/<profile>/`.
Unset, it resolves to `default`.

`~/.hub/repos/` sits outside the profile and is shared by all of them. A bare clone is a cache of a
remote and fetching into one is idempotent, so a second copy isolates nothing.

The variable holds one path segment rather than a path. That gives it a single construction path
with validation, the way `RepoSlug` and the other domain newtypes work, and it keeps every profile
under `~/.hub/` where a directory listing shows the whole set.

Every `just` recipe that runs from source defaults to `dev`. Installed binaries get `default` by
not having the variable set, which is also why a launchd daemon is unaffected by whatever a
developer's shell exports. The TUI names its profile in the status bar, so a run that isolated only
half of itself shows that in the first frame instead of never.

The existing database moves to `~/.hub/default/hub.db` through `maybe_migrate`
(`store/src/status_cache.rs:48`), the same mechanism that carried the last path move.

## Rejected

- **A root-level path variable covering all of `~/.hub/`** — because a root moves `repos/` with it,
  and copying 2.0 GB of bare clones isolates none of the 5.3 MB that actually collides. Reverses if
  `repos/` becomes per-instance state rather than a shared cache of remotes.
- **One variable per path, a database path plus a socket and a log path later** — because a run
  that sets three of four looks exactly like one that sets all four, and the symptom is production
  data in a dev session with nothing announcing it. Reverses if the paths stop being settable as
  one set.
- **One variable holding a path to the state directory, with `repos/` resolved separately** —
  because it delegates the layout to whoever sets the variable, so "where does dev state live" gets
  answered again in the justfile, in `daemon/README.md`, and in every doc describing a dev run,
  with nothing keeping those answers equal. Reverses if state has to live somewhere outside
  `~/.hub/`, a tempdir or a separate volume for instance.
- **Not yet, keeping the backup-and-restore workaround** — because Phase 3.3's `flock` refuses a
  second daemon outright, so the dev loop breaks at the next issue rather than at some unbounded
  point later. Reverses if Phase 3.3 is dropped or moved after Phase 5.

## Risk

A profile name nobody has used creates an empty profile rather than failing, because that is also
how a new profile is made. A typo therefore yields an empty database, not an error. What catches it
is that the recipes print the path they are opening, so the mistake is visible in the same second.
Validation cannot do this job: a typo and a new profile are the same input.

Isolation covers state, not the network. A TUI on the `dev` profile makes real API calls with real
credentials and spends real rate limit.

`just` supplies the variable per invocation. A developer who exports `HUB_PROFILE=dev` in their
shell hands it to the installed binary as well. The status bar makes that visible and nothing
prevents it.

`store/src/status_cache.rs` ends up carrying two migrations. Retiring `legacy_db_path` is separate
work this record does not schedule.

## Revisit when

A profile is wanted for something that cannot live under `~/.hub/`, a per-case tempdir in tests
being the likely one. Holding a name rather than a path is exactly what forecloses that, so it is
the observation that reopens this.

## Also update

- [x] questions/README.md — no open question closes into this record.
- [x] vision.md — says nothing about where hub stores state; nothing to change.
- [x] [014](014-task-dispatch.md) — its superseded banner names `~/.hub/hub.db` as the surviving
      data home; the banner now points here for the path.
