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
// config/src/lib.rs — credentials resolved once at startup
pub struct Config {
    pub github_token: Secret<String>,
    pub github_username: String,
    pub linear_token: Option<Secret<String>>,
    // ...
}

impl Config {
    pub async fn load() -> Result<Self> {
        let hub_toml = toml::parse_file("hub.toml")?;
        validate_required(&hub_toml.credentials)?;
        let github_token = Secret::new(resolve(hub_toml.credentials.github_token).await?);
        // ...
    }
}
```

`Secret<String>` (from the `secrecy` crate) flows through `Config`, `LokiEnv`,
and `StatusParams`. `.expose_secret()` is called only at client call sites —
every access point is explicit and grep-able. `Debug` output prints `[REDACTED]`,
preventing accidental logging.

`config::Config::load()` is called once in `main()` and the struct is passed
down as an argument. Nothing else calls `op read` or knows about 1Password.

## Per-device config

When hub-private is in use, the `[credentials]` table lives in
`hub-private/devices/<device>.toml`, which is symlinked to `hub/hub.toml`.
Each device declares only the credentials it needs; unknown keys in
`[credentials]` are captured in `extra_credentials` and passed to
hub-private workflows without the public hub code ever seeing the key names.
