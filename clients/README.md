# clients

External API wrappers. The only code that makes network calls.

**Rules:**
- One subdirectory per external service (github/, notion/, plex/, etc.)
- Adapts external API responses into domain types
- Never imports from store or workflows

**Lives here:** HTTP clients, auth handling, rate limit logic, response mapping.
