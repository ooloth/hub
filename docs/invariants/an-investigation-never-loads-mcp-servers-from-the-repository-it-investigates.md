# An investigation never loads MCP servers from the repository it investigates

## The invariant

Every investigation hub launches gets exactly the MCP servers listed under
`[investigation.mcp_servers]` in the device's `hub.toml`, and none from any other MCP config.

## Why it must hold

An MCP server entry is a command or a URL that Claude Code starts or connects to on the session's
behalf. A repository's `.mcp.json` is written by whoever wrote the repository, and for a PR
investigation that can be a stranger, since the worktree is the PR's own branch. A server from that
file runs with the session's access: on the machine holding the credentials, in a session nobody is
necessarily watching.

Claude Code asks before it loads servers from a project's `.mcp.json`, once per directory. Every
investigation worktree is a new directory, so without this invariant every investigation opens on
that approval prompt. The session then looks like it is running while it waits on a prompt in a
tmux window, and approving the prompt is the very act the invariant exists to prevent.

## What it forbids

- Launching an investigation without `--strict-mcp-config`, which is what makes Claude Code ignore
  the repository's `.mcp.json` and every other MCP config.
- Setting `enableAllProjectMcpServers`, or listing project servers in `enabledMcpjsonServers`, for
  investigation sessions. Either approves the repository's servers without a prompt, which is the
  same exposure with no chance to refuse it.
- Forwarding the user's own MCP config into investigations wholesale. The device chooses each
  server by name in `hub.toml`, so a server added to `~/.claude.json` for other work does not reach
  investigations without a decision.
- A second code path that launches an investigation's `claude` process other than `compose()` in
  `ui/tui/src/investigations/command.rs`.

## How it is enforced

Two tests in `ui/tui/src/investigations/command.rs` hold it up, and `just test` runs them:

- `investigations_ignore_every_mcp_config_hub_did_not_pass` asserts that the launch shell contains
  `--strict-mcp-config`, with and without configured servers.
- `a_device_with_mcp_servers_passes_them_through_the_environment` asserts that the configured
  servers, and only those, reach `--mcp-config` through `HUB_MCP_CONFIG`.

The snapshots in `ui/tui/src/investigations/snapshots/` pin the whole launch command for each
investigation type. `config/src/investigation.rs` rejects a server entry hub cannot validate.

What the checks miss:

- **Another launcher.** The tests cover `compose()`. Nothing fails if new code builds a `claude`
  command some other way. Today `compose()` is the only place in hub's Rust code that does.
- **Claude Code's own behaviour.** The tests prove hub passes the flag, not that Claude Code honours
  it. A live run with Claude Code 2.1.296 showed it does: a worktree containing a `.mcp.json` with
  two servers opened with no MCP prompt, and `/mcp` listed only the configured server.
- **MCP servers defined by agents.** `/mcp` in an investigation still lists the servers that agent
  definitions declare, marked agent-only. Those seen so far come from user-level agents. Whether a
  repository's own `.claude/agents/` can add servers this way under `--strict-mcp-config` is
  unverified.
- **What the agent does with its own tools.** An agent that runs `claude mcp add`, or starts
  another `claude` process, is outside what hub launches. Containing that is the subject of
  [the permissions question](../questions/should-investigations-run-with-permissions-skipped.md).
