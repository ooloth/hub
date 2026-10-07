---
name: next
description: Recommend what to work on next in hub, ranked by cost of delay. TRIGGER at the start of a session, or when the user asks what to work on next and you are not already partway through something.
---

Recommend one to three things to work on next, say why, and stop. The user decides.

## Cost of delay decides

Every item is ranked by its cost of delay: the pain, slowness, loss or harm that users and
developers meet on each day the problem stays, times how often they meet it. How long the work
would take is not part of it.

Each kind of work has a cost of delay that behaves predictably, which is why kind sets the default
order:

- **Work in flight** costs developers every day it waits: context is lost, branches go stale, and
  conflicts accumulate, while nothing it was meant to deliver reaches anyone.
- **Bugs** cause pain or harm each time someone meets them.
- **Started features** leave users without something already partly built, and the partial work
  decays like work in flight.
- **Maintenance** problems slow or mislead every later session, so fixing them before new work
  starts spares that work the cost.
- **New features** cost users the workaround they use each day without them.
- **Questions** cost nothing on their own. They carry the cost of delay of the work they block.

Kind is the default, and cost of delay is the rule it stands in for. An item moves across a kind
boundary only when you can say in one sentence why its cost of delay breaks the default, and that
sentence goes in the report. For example: "#253 shows a wrong key hint, which costs a second of
confusion and harms nothing, so it ranks after Phase 3.4."

In hub, the cost of delay is high when:

- hub shows a wrong signal or misses one, because ranking signals correctly is the product, and the
  notifications carry the same failure to the user
- a credential, or anything else the user did not mean to share, can leave the machine
- the cache, an investigation session or a worktree can be lost

## Where work lives

- **In flight:** `git status`, the current branch, and open PRs (`gh pr list`). A branch whose PR
  was closed unmerged is not in flight; report it for deletion.
- **Issues:** the GitHub tracker. Kind labels are `bug`, `feature` and `maintenance`. Invoke
  `use-gh` before any `gh` call, and pass `--limit 300` to every `gh issue list`, because its
  default of 30 truncates without saying so.
- **Milestones:** the current milestone is the open one with at least one closed issue. If two
  qualify, the `M1:` / `M2:` prefix in their titles orders them. If none qualifies, ask which
  milestone is current rather than picking one. List them with
  `gh api 'repos/ooloth/hub/milestones?state=open'`, quoted so the shell does not expand the `?`.
  Inside a milestone, the `Phase N.M` numbering in issue titles is the work order, and every issue
  in the milestone ranks by its phase number, defects included.
- **Started features outside a milestone:** a parent issue with some of its sub-issues closed.
  `gh issue view N --json subIssues,blockedBy` prints the relationships, each as an object with
  `nodes` and `totalCount` rather than as an array.
- **Open questions:** `docs/questions/`, one file per question. Issues cite the question files they
  depend on; the question files do not list what they block.
- **Not ranked:** an issue labelled `status:agent-working` already has an agent on it, unless the
  PR for it is closed, in which case report the label as stale. The `status:*` labels belong to
  the agent PR workflows and say nothing else about priority.

## Default order

1. **Resume:** uncommitted changes, a branch other than `main`, or an open PR.
2. **Label issues with no kind.** An unlabelled issue cannot be ranked, and a bug among them goes
   unranked. Propose `bug`, `feature` or `maintenance` for each from its title. Propose closing it
   as obsolete when the code it describes has changed, saying what you checked, or as a duplicate,
   naming the issue it duplicates. Read the body only when the title leaves the kind unclear or
   suggests the issue may be obsolete. Apply nothing until the user approves the batch. Milestone
   phases get a kind too, and still rank by their phase number.
3. **Bugs outside the current milestone**, by how often each one hits and how bad it is when it
   does.
4. **Started work:** the lowest open phase of the current milestone, then started features outside
   a milestone.
5. **Maintenance.**
6. **New features**, and the next milestone once the current one is finished.
7. **Questions no issue cites.** Attach each one to the issue it blocks, which moves it up the
   order, or delete it as obsolete. Answering a question that nothing needs yet decides it before
   the information that would improve the answer exists.

**If a top candidate depends on a decision nobody has made, that decision comes first**, at the
candidate's rank, and the report names the work it unblocks. Check this only for the candidates you
are about to recommend, never across the whole tracker. An unmade decision appears in two places:
a `docs/questions/` file the issue cites, or an entry under **Open** in the issue's **Decisions**
section. Read the cited file and the issue's comments to judge whether the dependency is real,
since a citation alone can be background.

Before recommending an issue, confirm against the code that the problem it describes still
exists. An issue whose problem is gone, or sits in code nothing calls, has no cost of delay.

Read issue comments and relationships rather than bodies when deciding whether work has started:
`gh issue view N --json state,comments,blockedBy`.

## Report

For each recommendation, three or four lines: the item, its rank, and the sentence of evidence that
decided it, including any cost-of-delay sentence that moved it across a kind boundary. Name
anything ambiguous, such as two milestones that both qualify as current, rather than picking. If
nothing is left anywhere, say so and offer `scan-gaps`.
