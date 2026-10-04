# Set Up the Private Workflows Repository

Steps to wire `hub-private` into `hub` on a device. Run this once after cloning —
or again on any new machine to restore private workflows from the existing repo.

## 1. Clone both repositories

Clone both repositories side by side in one parent directory, and run the commands in the
rest of this playbook from that directory unless they say otherwise.

```bash
git clone git@github.com:ooloth/hub.git
git clone git@github.com:ooloth/hub-private.git
```

Skip whichever repo you already have.

Install hub-private's own pre-commit hook (see [CONTRIBUTING.md](../../CONTRIBUTING.md)):

```bash
(cd hub-private && prek install)
```

## 2. Add a device config (new devices only)

If this device doesn't have a config file yet, copy the closest existing one and edit it:

```bash
cp hub-private/devices/home-laptop.toml hub-private/devices/<this-device>.toml
```

Add the `[credentials]` entries and `[[project]]` blocks relevant to this machine.
See [Add a Project](add-a-project.md) for the project config format.

## 3. Wire the symlinks

```bash
cd hub
just setup
just setup-private <this-device>
just build
```

`just setup` installs the cargo tools and git hooks. `just setup-private` creates symlinks for
the private clients, workflows, and device config (`hub.toml`). If a regular `hub.toml` file
is already in the repo root, it stops without touching that file. Remove or move the file
and run it again. `just build` produces the `scripts` binary that the `gh` guard hook runs.

## 4. Verify

```bash
just check
```

See [Private Workflows](../architecture/private-workflows.md) for how the two-repo
model works.
