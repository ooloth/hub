---
name: repo-scan
description: Scan repos listed in hub.toml for a given issue category and file well-defined GitHub issues for findings.
allowed-tools: [Bash]
effort: high
model: opus
---

## Purpose

Scans every repo listed in hub.toml for a named issue category, surfaces ranked findings grouped by repo, and files GitHub issues for confirmed findings — with duplicate detection and label management handled automatically.

## Prerequisites

- `gh` CLI authenticated (`gh auth status`)
- Install: `brew install gh`

## Theme lookup

| Theme | Reference file | Finds |
|---|---|---|
| `docs` | `references/docs.md` | Stale, drifted, inconsistent, or missing documentation |

Load the reference file for the requested theme before scanning. It defines what to look for, what to skip, and how to rank findings.

## Config

Reads `name` and `repo` from each `[[project]]` in hub.toml. No additional fields required.

```toml
[[project]]
name = "hub"
repo = "ooloth/hub"
```

`name` is used as the human label in output and as the local clone path (future). `repo` is the GitHub slug used for all `gh api` calls.

## Starting queries

List all file paths in a repo:

```bash
gh api repos/{owner}/{repo}/git/trees/HEAD?recursive=1 \
  --jq '[.tree[] | select(.type == "blob") | .path]'
```

Read a specific file:

```bash
gh api repos/{owner}/{repo}/contents/{path} \
  --jq '.content' | base64 -d
```

List open issues (for dedup):

```bash
gh issue list --repo {owner}/{repo} --state open --limit 100 \
  --json number,title,body
```

## Workflow

Work through every repo in hub.toml in sequence.

### 1. Enumerate doc surfaces

Using the file tree, identify the surfaces defined in the theme reference file. Read each one.

### 2. Apply heuristics

Apply the heuristics from the reference file. For each finding, record:
- **File**: the path of the doc that has the issue
- **Tier**: the severity tier from the reference file
- **Finding**: one sentence describing the specific problem
- **Evidence**: the exact text or reference that is wrong/missing

Skip anything the reference file marks as a known false positive.

### 3. Present findings

After scanning all repos, output findings grouped by repo, ordered within each group by tier (highest first). Format:

```
── ooloth/hub ──────────────────────────────────────────
[broken-ref]    docs/playbooks/add-a-workflow.md
                References `clients/github/mod.rs` — file does not exist

[drift]         README.md
                Says "run `cargo run --bin hub`" — current entry point is `just cli`

── ooloth/dotfiles ─────────────────────────────────────
[gap]           No README found
```

Pause here. Ask the user which findings to file as issues. The user may confirm all, select individual findings, or skip a finding with a reason.

### 4. Dedup

For each confirmed finding, search open issues before filing:

```bash
gh issue list --repo {owner}/{repo} --state open --limit 100 \
  --json number,title \
  --jq '.[].title'
```

Compare semantically — same problem, different title still counts as a duplicate. If a duplicate exists, note its number and skip filing.

### 5. Ensure labels exist

Before filing the first issue in a repo, create the three labels if they are missing:

```bash
gh label create "author:agent"            --color "0075ca" --repo {owner}/{repo} --force
gh label create "category:docs"           --color "e4e669" --repo {owner}/{repo} --force
gh label create "status:needs-human-review" --color "d93f0b" --repo {owner}/{repo} --force
```

`--force` is idempotent — safe to run even if the label already exists.

### 6. Draft and file

For each non-duplicate confirmed finding, draft an issue body following the `write-ticket-description` template: Why, Current state, Ideal state, Out of scope (if needed), Starting points, QA plan, Done when.

- **Starting points**: the specific file(s) where the problem was found — not directories
- **QA plan**: steps a reader can follow to verify the fix is correct by inspection (not by running tests)
- **Done when**: one sentence — the doc is accurate, or the gap is filled

File the issue:

```bash
gh issue create \
  --repo {owner}/{repo} \
  --title "{title}" \
  --body "{body}" \
  --label "author:agent,category:docs,status:needs-human-review"
```

### 7. Report

After all filings are done, output a summary:

```
Filed:
  #42  ooloth/hub         — broken ref in docs/playbooks/add-a-workflow.md
  #17  ooloth/dotfiles    — no README

Skipped (duplicate):
  ooloth/hub  — drift in README.md  →  already tracked in #38

Skipped (user):
  ooloth/hub  — gap: no ADR for config model
```
