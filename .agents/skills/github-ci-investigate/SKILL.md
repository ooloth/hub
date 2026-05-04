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

## Invocation

Two modes depending on whether arguments are provided:

**With arguments** (launched from the TUI via `i`):
```
/github-ci-investigate <repo> <run-url>
```
- `<repo>` — `org/repo` slug
- `<run-url>` — full GitHub Actions run URL (e.g. `https://github.com/org/repo/actions/runs/12345678`)

Extract the run ID from the URL (`runs/<id>`), then skip straight to Step 2 of the investigation pattern below.

**Without arguments** (manual invocation): fall through to Step 1 below.

## Config (manual mode only)

Reads `repo` from the active hub.toml project:

```toml
[[project]]
name = "my-app"
repo = "org/my-app"      # used as the --repo argument to gh

[[project.workflow]]
name = "github-ci"
```

## Starting queries (manual mode only)

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

1. **Orient** *(manual mode only)* — resolve the default branch, then list the last 10 failed runs on it; pick the most recent one. Note the workflow name and `databaseId`.

2. **Identify the failed step** — get structured failure info first (which step failed), then grep for the error:

   ```bash
   # Which step failed?
   gh run view <id> --repo <repo> --json name,conclusion,headBranch,headSha,createdAt,jobs \
     --jq '{name,conclusion,headBranch,headSha: .headSha[0:8],createdAt,jobs: [.jobs[] | select(.conclusion == "failure") | {name,conclusion,steps: [.steps[] | select(.conclusion == "failure") | {name,conclusion}]}]}'

   # Extract the error lines (skip hundreds of lines of runner boilerplate)
   gh run view <id> --repo <repo> --log-failed 2>&1 | grep -E 'error\[|^Error|error:' | head -20
   ```

   Only fall back to the full `--log-failed` output if the grep returns nothing useful.

3. **Find the regression boundary** — if the failure looks like a config/dependency issue rather than a code bug, immediately find where it started:

   ```bash
   # Find last success and first failure
   gh run list --repo <repo> --branch <branch> --limit 30 \
     --json databaseId,conclusion,createdAt,headSha \
     --jq '.[] | "\(.databaseId) \(.conclusion) \(.createdAt) \(.headSha[0:8])"'

   # Diff the commits between last success and first failure
   gh api "repos/<repo>/compare/<last-success-sha>...<first-failure-sha>" \
     --jq '[.files[] | .filename] | join("\n")'
   ```

4. **Form a hypothesis** — based on the error and the regression boundary, decide what to look at next: a specific step's full log, the diff that introduced the failure, or a recent commit that changed a related file.

5. **Validate** — run a targeted follow-up query. Examples:
   - `gh run view <id> --log` for the full log if `--log-failed` is truncated
   - `gh run view <id> --repo <repo> --log 2>&1 | grep -A 200 "<step command>"` to get a specific step's output
   - `gh api repos/<repo>/commits?sha=<default-branch>&per_page=5` to inspect recent commits around the failure time

6. **Stop when** you can name the failing step, the error message, and a likely cause. Three iterations is usually enough; if not, surface what you know and flag what needs a human look.

## Output format

Use headers and bullets, not a code block:

**Failed:** `<step name>` — `<job name>` job, `<step name>` step, commit `<sha>` ("<commit title>")

**Cause:** One or two sentences explaining why it failed.

**Options:**
1. First fix option — when to use it
2. Second fix option — when to use it (if applicable)

If the cause is genuinely ambiguous, say so explicitly rather than guessing. Omit **Options** and replace with **Next:** if there is only one clear fix.
