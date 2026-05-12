## Purpose

Deeply understand a GitHub issue and recommend how to handle it:

- **Ready for agent** — scope is clear, acceptance criteria are unambiguous, no design decisions remain: apply `status:ready-for-agent` to queue it for autonomous work
- **Split** — too broad to hand to an agent as-is, but cleanly decomposable: draft child issues and file them on confirmation
- **Interactive** — requires design discussion, architectural judgment, or tradeoff decisions that can't be pre-resolved: offer to start on it together right now

## Prerequisites

- `gh` CLI authenticated (`gh auth status`)
- Running in a worktree for the target repo (hub TUI ensures this)

## Context

Launched from the hub TUI with:
- `repo` — `org/repo` slug
- `number` — GitHub issue number

## Investigation pattern

### 1. Fetch the issue

```bash
gh issue view <number> --repo <repo> \
  --json number,title,body,labels,comments,createdAt,updatedAt,state,author
```

Read the title, body, and comments in full. Note the labels — if `status:ready-for-agent` is already present, say so and stop.

### 2. Understand the codebase context

Using `rg`, `fd`, and `Read`, explore the areas the issue touches:

- Find files and symbols mentioned in the issue body
- Check the surrounding code and module structure
- Identify which crates are affected
- Check whether the violation or defect the issue describes still exists in the current code

### 3. Assess staleness

Use your judgment across all available signals: the code state, git history since the issue was filed, and any comments. Ask yourself:

- Has the root cause already been fixed?
- Have the files it references changed in ways that make the issue description inaccurate?
- Do the comments suggest resolution or changed direction?

If the issue is stale, say so explicitly and explain why. Stop here — don't recommend a path for a resolved issue.

### 4. Estimate effort and ROI

Think holistically. Consider:

- How many files and crates need to change?
- Are there schema, serialization, or cache version implications?
- Does the fix require new tests, or do existing tests cover it?
- What's the blast radius if the fix is wrong?
- What's the cost of *not* fixing it — correctness bugs, tech debt accumulation, agent failure modes?

Produce a brief, honest estimate: S/M/L effort, Low/Medium/High ROI.

### 5. Recommend a path

**Ready for agent** if:
- The scope is bounded and acceptance criteria are clear from the issue body alone
- No design decisions are left open (the *how* is as obvious as the *what*)
- An agent could identify the files to change and make the fix without human guidance

**Split** if:
- The issue contains multiple distinct changes that could each be agent-ready independently
- One part is clear but another requires discussion — split them so the clear part can move

**Interactive** if:
- The fix requires weighing architectural tradeoffs
- Acceptance criteria depend on design decisions not yet made
- The issue is vague enough that misinterpreting it would waste significant effort

### 6. Act on the recommendation

**If ready for agent:**

Confirm with the user, then apply the label (creating it first if absent):

```bash
# Create label if it doesn't exist yet
gh label list --repo <repo> --json name --jq '.[].name' | grep -q "status:ready-for-agent" \
  || gh label create "status:ready-for-agent" --repo <repo> \
       --color "0E8A16" --description "Ready for an autonomous agent to implement"

# Apply the label
gh issue edit <number> --repo <repo> --add-label "status:ready-for-agent"
```

**If split:**

Draft each child issue with a concise title, a body that stands alone (context, acceptance criteria, affected files), and any relevant labels. Show the drafts and ask for confirmation before filing:

```bash
gh issue create --repo <repo> --title "..." --body "..." --label "..."
```

**If interactive:**

Explain clearly what makes this a human collaboration: the open design questions, the tradeoffs that need to be weighed, or the ambiguity that needs resolving. Then ask:

> Want to start on it together now? I can walk through the tradeoffs and we can implement once we've aligned.

If yes, begin the discussion.

## Output format

Lead with a one-line verdict, then expand:

**Verdict:** [Ready for agent | Split into N issues | Interactive]
**Staleness:** [Still valid | Stale — reason]
**Effort:** S / M / L
**ROI:** Low / Medium / High

**Assessment:** 2–4 sentences on what the issue asks for, what the code actually looks like, and why you reached this verdict.

**Next step:** what you're about to do (apply label / show issue drafts / ask to start together).
