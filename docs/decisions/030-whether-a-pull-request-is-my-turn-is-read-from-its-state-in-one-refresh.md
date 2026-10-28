---
number: 030
status: accepted
date: 2026-10-24
---

# 030 — Whether a pull request is my turn is read from its state in one refresh, never from what happened since a push

## Forced by

- [#370](https://github.com/ooloth/hub/issues/370): agents open pull requests under my account in
  my own repositories, nobody else reviews them, and nothing makes them my turn.
- Agents also reply and comment under my account, and hub cannot tell their actions from mine. On
  ooloth/hub#161 the agent pushed at `01:09:21` and replied to Copilot's two threads as me at
  `01:09:32` and `01:09:34` (repository activity and review times, read 2026-10-23). A rule that
  counts "a reply of mine since the push" ends that turn 11 seconds after it starts.
- [#371](https://github.com/ooloth/hub/issues/371)'s rule, as shipped in c823c4e, is event-based:
  feedback waits on me when it came after my last push and after my last reply. One reply anywhere
  answers every thread, and a push answers feedback nobody has read
  ([#372's comment of 2026-10-20](https://github.com/ooloth/hub/issues/372)). The push time needs
  a REST lookup per pull request, because GraphQL does not say when a commit was pushed
  ([constraints](../constraints.md)), and a failed lookup leaves the pull request undecided.
- The account holder, 2026-10-23: "I feel like we're overcomplicating this by thinking action by
  action instead of just looking at the pr holistically and deriving what state it's in", and, on
  whether a push should end a turn, "it's not the push, it's the facts about the pr".
- [020](020-hub-runs-an-unattended-surface.md): a notification interrupts, so a turn has to end
  once I have acted, and has to begin without me opening anything.

## Decision

Whether a pull request is my turn, and why, is a function of what one refresh read about it from
GitHub. That covers its state and draft flag, its pending review requests, each review thread
(whether it is resolved, who commented in it, and whose comment is last), each reviewer's latest
verdict and GitHub's review decision, its check rollup and mergeability, and the order of the
top-level comments and review bodies relative to mine. Nothing is compared with the time of a push.
Nothing is carried from one refresh to the next to work out what changed. Two refreshes that read
the same state give the same answer, whichever of them ran first and however many were missed in
between.

Feedback is answered in the place it was left. A thread is answered by a reply in it or by
resolving it. A verdict is answered when its reviewer is asked again or gives a new one. A review
body or top-level comment that has no thread is answered by something of mine that comes after it
in the same pull request. That last case is the one place order matters, and it is order within the
snapshot, not time against a push.

The rules that turn this state into a reason, such as which reason comes first or how a draft is
treated, are not part of this record. They can change inside `TurnReading::of` without touching
what is fetched, and they are recorded in the issue that builds each one, starting with #370.

The repository activity lookup goes, along with the undecided state a failed lookup causes, so a
refresh makes one request fewer for each pull request that needed one. The query has to read the
newest comments in each review thread rather than the first 10, which makes each search response
larger. No new store is added: the facts are cached on each pull request in the existing cache row,
whose schema version moves.

## Rejected

- **Event-based, against the last push (#371 as shipped).** Its strongest case is that it reads
  the feedback with no thread to answer in, such as a review body, by the most natural signal: did
  anything happen after it. It is rejected because it answers feedback from the wrong place. One
  reply clears every thread, and a push clears feedback nobody has read, so an agent's push and
  reply under my account end the turn with nobody having looked (hub#161). It would reverse if a
  kind of feedback appeared that has neither a resolved state nor a place in the pull request's
  order.
- **Diffing refreshes: storing what the last refresh saw and making a turn from what changed.** Its
  strongest case is that it can tell a new reply from an old one even where GitHub records no
  resolved state. It is rejected because a refresh that fails or is missed loses the change for
  good, and a new profile or an emptied cache has nothing to diff against, so the same pull request
  would get different answers on different machines. It would reverse if a turn needed something
  GitHub does not keep.
- **GitHub's notifications API as the source of events.** Its strongest case is that GitHub already
  decides what concerns me. It is rejected because GitHub does not notify me of what my own account
  does, so a pull request an agent opens under my account, the case #370 exists for, never appears
  in it. Notifications also record events, not whose move it is.
- **Not yet: keep #371's rule and add #370 as one more rule against the push.** That was the first
  approach drafted for #370. It is rejected because it inherits #371's blind spot: a thread reply
  an agent leaves under my account after its push ends the new turn too.

## Risk

Hub still cannot tell an agent's action under my account from mine. Reading state limits the damage
to the thread where the agent replied, rather than every piece of feedback on the pull request, but
a reply an agent leaves in a person's thread still reads as mine.

A review body or top-level comment can only be answered by order, so any comment of mine after it,
an agent's included, answers it.

Each search response grows, because every review thread brings its newest comments. GitHub limits
how many nodes a GraphQL query may ask for, and a refresh that exceeds the limit fails as a whole.

What one query reads is bounded, and the bounds are where the reading can be wrong. Only the newest
50 threads of a pull request are read, so feedback in an older unresolved thread is missed. Who
took part in a thread is read from its newest 10 comments, so a person who spoke only before them
no longer holds the thread, and the pull request can read as `no reviewer`. And an approved pull
request that GitHub reports as blocked while a check is running reads as ready to merge, because the
merge state does not say whether the running check is what blocks it; if something else does, the
turn ends again once the check finishes.

## Revisit when

A refresh fails with a GraphQL node or complexity limit error while reading review threads. Or a
turn is needed that GitHub's state cannot show, such as one that depends on something I did in hub
itself. Hub-local state such as a snooze is not ruled out by this record. It would sit on top of
the reading, not replace it.

## Also update

- [x] questions/README.md — nothing settles or changes scope. [Should a waiting pull request be
      snoozable?](../questions/should-a-waiting-pull-request-be-snoozable.md) stays open, and this
      record leaves room for its answer
- [x] vision.md — nothing defined or foreclosed; it does not describe how a turn is worked out
- [x] constraints.md — the repository activity entry stays, as a measured fact, and now says
      nothing in hub uses it
