---
name: github-ci-investigate
description: Diagnose why a GitHub Actions workflow run failed and surface the root cause. Reads hub.toml for repo context.
allowed-tools: [Bash]
effort: high
model: opus
---

## Purpose

Diagnoses why a GitHub Actions workflow run failed and surfaces a root cause with a suggested next action.

## Prerequisites

- `gh` CLI authenticated (`gh auth status`)
- Install: `brew install gh`

## Config

Reads `repo` from the active hub.toml project:

```toml
[[project]]
name = "my-app"
repo = "org/my-app"      # used as the --repo argument to gh

[[project.workflow]]
name = "github-ci"
```

## Starting queries

Resolve the default branch (could be `main`, `master`, `trunk`, etc.):

```bash
gh repo view <repo> --json defaultBranchRef --jq '.defaultBranchRef.name'
```

List recent failed runs on the default branch:

```bash
gh run list --repo <repo> --branch <default-branch> --status failure --limit 10 \
  --json databaseId,name,conclusion,createdAt,headBranch,url
```

Fetch the failed step logs for the most recent run (use `databaseId` from above):

```bash
gh run view <run-id> --repo <repo> --log-failed
```

## Investigation pattern

1. **Orient** — resolve the default branch, then list the last 10 failed runs on it; pick the most recent one. Note the workflow name and `databaseId`.

2. **Read the failure** — `gh run view <id> --log-failed` streams only the failed step output. Scan for the first error line or exception.

3. **Form a hypothesis** — based on the error, decide what to look at next: a specific step's full log, a prior run to check for regression, a recent commit that changed a related file.

4. **Validate** — run a targeted follow-up query. Examples:
   - `gh run view <id> --log` for the full log if `--log-failed` is truncated
   - `gh run list --repo <repo> --workflow <filename> --limit 20 --json conclusion,createdAt` to see when runs started failing
   - `gh api repos/<repo>/commits?sha=<default-branch>&per_page=5` to inspect recent commits around the failure time

5. **Stop when** you can name the failing step, the error message, and a likely cause. Three iterations is usually enough; if not, surface what you know and flag what needs a human look.

## Output format

```
Repo:     org/my-app
Workflow: ci.yml
Run:      #12345 — https://github.com/org/my-app/actions/runs/12345
Failed:   "Run tests" step
Error:    <paste the key error line>
Cause:    <one sentence — e.g. "dependency version mismatch introduced in commit abc1234">
Next:     <one action — e.g. "pin dep to 1.2.3 or update test fixture">
```

Keep the diagnosis to the seven fields above. If the cause is genuinely ambiguous, say so explicitly rather than guessing.
