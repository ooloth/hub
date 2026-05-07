---
name: sonarr-investigate
description: Diagnose why a Sonarr download is import-blocked and surface the cause with suggested manual steps.
allowed-tools: [Bash]
effort: medium
model: sonnet
---

## Purpose

Diagnoses why a specific Sonarr episode is stuck in an import-blocked state and
explains what to do about it. Currently diagnose-only; the intent is to expand
this to auto-fix (blacklist the release and trigger a re-search) once the
diagnose path is trusted.

## Prerequisites

- `SONARR_URL` and `SONARR_API_KEY` available in the environment (injected by
  `op run` when launching via the TUI)

## Invocation

Launched from the TUI via `i` on a blocked item in the Errors category:

```
/sonarr-investigate <title> -- <error>
```

- `<title>` — series name and episode, e.g. `House of Cards (US) — S03E12`
- `<error>` — the Sonarr status message, e.g. `Invalid video file, unsupported extension: '.exe'`

## Investigation pattern

1. **Classify the error** from the `<error>` argument — most blocked imports
   fall into one of these categories:

   | Error pattern | Meaning |
   |---|---|
   | `Invalid video file, unsupported extension` | The download is not a video (`.exe`, `.scr`, etc.) — malware or wrong release |
   | `Not an upgrade for existing episode file(s)` | Sonarr already has a file at equal or higher quality; auto-import is blocked by cutoff |
   | `Unable to determine if file is a sample` | Sonarr cannot tell whether the file is a sample; needs manual review |
   | `Found matching series via grab history, but release was matched to series by ID` | Sonarr matched the release by ID rather than by name; automatic import is disabled as a safety check |

2. **Fetch the blocked queue entry** from Sonarr for additional context:

   ```bash
   curl -s -H "X-Api-Key: $SONARR_API_KEY" \
     "$SONARR_URL/api/v3/queue?pageSize=1000&includeSeries=true&includeEpisode=true" \
     | jq '[.records[] | select(.trackedDownloadState == "importBlocked") | {
         series: .series.title,
         episode: "\(.episode.seasonNumber)x\(.episode.episodeNumber)",
         title: .title,
         size: .size,
         statusMessages: [.statusMessages[].messages[]]
       }]'
   ```

   Filter the output for the series/episode that matches `<title>` to get the
   download title and any additional status messages.

3. **Form a diagnosis** — combine the error classification with any additional
   context from the queue entry (download title, file size, extra messages).

4. **Stop when** you can name the specific cause and give the user clear manual
   steps. Do not guess.

## Output format

**Blocked:** `<series> <episode>` — `<error summary>`

**Cause:** One or two sentences explaining why import is blocked.

**Steps:**
1. First manual action — what to do in Sonarr and where to find it
2. Second action if applicable

If the error category is unrecognised, say so and include the raw status
messages so the user can investigate directly.
