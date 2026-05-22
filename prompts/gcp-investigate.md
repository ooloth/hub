## Purpose

Diagnoses a GCP Cloud Logging error by finding where it originates in the codebase, correlating with recent changes, and recommending a fix.

## Prerequisites

- Running in the project's local worktree (hub TUI ensures this)

## Context

Launched from the hub TUI with:
- `project` — project name (e.g. "mapapp")
- `env` — environment (e.g. "neuro", "prod")
- `title` — error category (e.g. "errors")
- `message` — extracted message label (e.g. "something broke")
- `lines` — compact JSON array of raw log entries from this alert window; one element for a single occurrence, multiple for a grouped alert
- `lookback` — the time window these entries were fetched from (e.g. "1h")
- `url` — GCP Cloud Logging Console URL with the query and time range pre-encoded; open it to browse additional entries interactively

## Investigation pattern

### 1. Fetch more log context

Use the console URL to browse additional entries, or query the API directly. Extract the GCP project ID and filter from the URL's `project=` and `query=` parameters:

```bash
# Fetch recent entries matching the same filter (adjust --freshness to match lookback):
gcloud logging read '<filter-from-url>' \
  --project=<project-id-from-url> \
  --freshness=<lookback> \
  --limit=50 \
  --format=json | jq '.[] | .jsonPayload // .textPayload'
```

**Python/Flask traceback assembly:** Python tracebacks emit one stderr log entry per
line, so a single exception produces 10–30 sibling entries within a 0.05s window. If
your `lines` entries have `textPayload` (plain strings rather than structured JSON),
fetch a ±5s window from the same `pod_name` to assemble the full traceback before
tracing to source:

```bash
# Substitute the pod_name from any entry's resource.labels.pod_name:
gcloud logging read 'resource.labels.pod_name="<pod-name>" AND timestamp>="<ts-5s>" AND timestamp<="<ts+5s>"' \
  --project=<project-id-from-url> \
  --format=json | jq -r '.[].textPayload // .[].jsonPayload.message' | grep -v '^$'
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

Look for changes in the last few commits that could explain a new or changed error
pattern. If `git log` fails (e.g. due to a misconfigured `GIT_CONFIG_PARAMETERS`),
skip this step and note it in the output rather than retrying.

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
