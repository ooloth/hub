# Secrets

Credentials live in `hub.toml`'s `[credentials]` table, alongside project
and workflow config. Values are either plain strings or 1Password references
(`op://Vault/Item/field`). The binary resolves references at startup by
shelling out to `op read`; plain values pass through unchanged.

```toml
# hub.toml
[credentials]
github_token    = "op://Scripts/GitHub/personal-access-token"
github_username = "ooloth"
linear_token    = "op://Scripts/Linear/api-key"   # optional
loki_token      = "op://Scripts/Loki/token"        # optional
```

```rust
// config/src/config_file.rs — the file, read once; credentials resolved from it
impl ConfigFile {
    pub fn read() -> Result<Self> { /* parse hub.toml, check required keys */ }
    pub async fn resolve(&self, responsibility: OpResponsibility)
        -> Result<Resolved, CredentialFailure> { /* op read each reference */ }
}

// config/src/resolved.rs
pub struct Resolved {
    pub config: Config,
    pub spawns: OpSpawnsUsed, // how each op that answered was started
}

pub struct Config {
    pub github_token: Secret<String>,
    pub github_username: String,
    pub linear_token: Option<Secret<String>>,
    // ...
}
```

`Secret<String>` (from the `secrecy` crate) flows through `Config`, `LokiEnv`,
and `StatusParams`. `.expose_secret()` is called only at client call sites —
every access point is explicit and grep-able. `Debug` output prints `[REDACTED]`,
preventing accidental logging.

The TUI calls `config::Config::load()`, which reads the file and
resolves it in one step, once in `main()`, and pass the struct down as an
argument. Nothing else calls `op read` or knows about 1Password.

Every caller of `resolve` says which process macOS may hold responsible for the
`op` it starts (`config::OpResponsibility`). The TUI passes `Inherited`, so its
`op` answers to the terminal it runs in. The daemon passes
`InheritedUnlessRefused`: its `op` answers to `hub-daemon`, and a credential
1Password refuses with "No accounts configured" is read again through an `op`
that answers for itself, which is what lets it reach 1Password under launchd on
macOS 27, where the ordinary `op` was refused every time it was measured.
[`disclaimed_spawn`](../../disclaimed_spawn/README.md) explains how and why.

The daemon reads the file and resolves it as two steps
([Decision 027](../decisions/027-daemon-resolves-credentials-once-per-start-and-holds-them-in-memory.md)).
A broken `hub.toml` ends the start. Each `op read` has a 90-second
deadline, and an `op` still silent at the deadline is ended. A credential
that fails is a `CredentialFailure` naming its cause: a Touch ID prompt
nobody answered, a macOS prompt waiting for you, the app running but not
answering, the desktop app not running, 1Password unreachable, no 1Password
account visible to `op` ("No accounts configured"), or something else, with `op`'s message redacted of every credential resolved before it.
At the deadline the daemon reads which programs own the windows on screen,
through `osascript`, to tell those apart. The daemon records it and resolves again in
the same process, so a restart never raises fresh prompts. Credentials are
held in memory only, for the life of the process. The TUI gets the same
deadline through `Config::load`.

## Per-device config

When hub-private is in use, the `[credentials]` table lives in
`hub-private/devices/<device>.toml`, which is symlinked to `hub/hub.toml`.
Each device declares only the credentials it needs; unknown keys in
`[credentials]` are captured in `extra_credentials` and passed to
hub-private workflows without the public hub code ever seeing the key names.
