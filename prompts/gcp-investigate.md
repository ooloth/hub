## Purpose

Diagnoses a GCP Cloud Logging error by finding where it originates in the codebase, correlating with recent changes, and recommending a fix.

## Prerequisites

- Running in a fresh detached-HEAD worktree of the project repo (hub TUI creates it automatically from the last-fetched state of the default branch). Use `rg`, `Read`, and `git log` directly — no setup needed.

## Context

Launched from the hub TUI with:

- `project` — hub project name (e.g. "dash-phenoapp-v2")
- `env` — environment (e.g. "neuro", "prod")
- `gcp_project` — GCP cloud project ID (e.g. "rp006-prod-49a893d8"); use this as `--project` in `gcloud` commands and as the `project=` param in GCP Console URLs
- `title` — error category (e.g. "errors")
- `message` — extracted message label (e.g. "something broke")
- `lines` — compact JSON array of raw log entries from this alert window; one element for a single occurrence, multiple for a grouped alert
- `incident_at` — timestamp of the first log entry (ISO 8601); use this to anchor time-range queries (e.g. ±5 s window for traceback assembly)
- `lookback` — the time window these entries were fetched from (e.g. "1h")
- `url` — GCP Cloud Logging Console URL with the query and time range pre-encoded; open it to browse additional entries interactively

## Investigation pattern

### 1. Fetch more log context

**If `lines` entries have `textPayload` (plain strings rather than structured JSON), do this first.**
Python/Flask tracebacks emit one stderr log entry per line, so a single exception produces
10–30 sibling entries within a 0.05s window. Use `incident_at` to anchor the ±5s query:

```bash
# Substitute pod_name from any entry's resource.labels.pod_name, and incident_at as the anchor:
gcloud logging read 'resource.labels.pod_name="<pod-name>" AND timestamp>="<incident_at-5s>" AND timestamp<="<incident_at+5s>"' \
  --project=<gcp_project> \
  --format=json | jq -r '.[].textPayload // .[].jsonPayload.message' | grep -v '^$'
```

For structured (`jsonPayload`) entries, fetch recent entries matching the same filter:

```bash
# Fetch recent entries matching the same filter (adjust --freshness to match lookback):
gcloud logging read '<filter-from-url>' \
  --project=<gcp_project> \
  --freshness=<lookback> \
  --limit=50 \
  --format=json | jq '.[] | .jsonPayload // .textPayload'
```

### 2. Parse the log lines

Read the `lines` array for the full log context of each occurrence: stack trace,
request details, jsonPayload, or textPayload. Extract the most specific signal —
an exception type, a function name, a message prefix. If `jsonPayload` entries
include `_module`, `_func`, or `_lineno` fields, surface those first — they
identify the exact source location without a search.

### 3. Trace to source

Using `rg` and `Read`, find where in the codebase this error is raised or logged. Look for the exception class, the log message string, or the surrounding context. Read the relevant function and its callers.

### 4. Check recent history

```bash
git log --oneline -20 -- <affected-files>
git diff HEAD~5 -- <affected-files>
```

Look for changes in the last few commits that could explain a new or changed error pattern.

### 5. Form a hypothesis

Based on the error signal, the source location, and the recent history, name a likely cause. If multiple causes are plausible, rank them.

### 6. Validate

If the hypothesis is testable from the local codebase (a recent change, a clear logic bug, a missing guard), confirm it. If validation requires runtime access, name what you'd need to verify.

### 7. Stop when

You can name the error's origin, its likely cause, and at least one concrete next step. Three iterations is usually enough; if not, surface what you know and flag what needs a human look.

## Output format

**Error:** `<title>` — `<message>` in `<project>:<env>`

**Origin:** File and function where the error is raised or logged.

**Cause:** One or two sentences explaining why it's happening.

**Options:**

1. First fix — when to use it
2. Second fix — if applicable

If the cause is genuinely ambiguous, say so and list what additional signals would resolve it.
