# Set Up the Private Workflows Repository

Steps to wire `hub-private` into `hub` on a device. Run this once after cloning —
or again on any new machine to restore private workflows from the existing repo.

## 1. Clone both repositories

```bash
git clone git@github.com:ooloth/hub.git
git clone git@github.com:ooloth/hub-private.git ../hub-private
```

Skip whichever repo you already have.

## 2. Add a device config (new devices only)

If this device doesn't have a config file yet, copy the closest existing one and edit it:

```bash
cp hub-private/devices/home-laptop.toml hub-private/devices/<this-device>.toml
```

Add the `[[project]]` entries and workflows relevant to this machine. See
[Add a Project](add-a-project.md) for the config format.

## 3. Wire the symlinks

```bash
cd hub
just setup-private <this-device>
```

This creates symlinks for the private clients, workflows, device config, and `.env`.

## 4. Verify

```bash
just check
```

See [Private Workflows](../architecture/private-workflows.md) for how the two-repo
model works.
