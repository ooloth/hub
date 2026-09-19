#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///
"""Verify every workspace member opts into the workspace lints.

A crate without `[lints]\nworkspace = true` in its manifest is compiled without
the workspace's clippy and rustc settings, so it passes `just check` while
holding none of the standards the rest of the tree is held to. Nothing else
reports that, because a missing opt-in looks exactly like a clean crate.

    scripts/check-lint-inheritance.py

Exits non-zero and names each manifest that is missing the opt-in. Run by
`just check` and by the prek hook.
"""

from __future__ import annotations

import json
import pathlib
import subprocess
import sys


def manifests_missing_workspace_lints() -> list[str]:
    """Manifest paths for workspace members that do not inherit the lints."""
    try:
        metadata = subprocess.check_output(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"]
        )
    except FileNotFoundError:
        raise SystemExit("cargo is not on PATH; run this from a Rust toolchain shell")
    except subprocess.CalledProcessError as error:
        raise SystemExit(f"cargo metadata failed with exit code {error.returncode}")

    packages = json.loads(metadata)["packages"]
    return [
        package["manifest_path"]
        for package in packages
        if "workspace = true" not in pathlib.Path(package["manifest_path"]).read_text()
    ]


def main() -> None:
    missing = manifests_missing_workspace_lints()
    if not missing:
        return
    print("ERROR: missing [lints]\n  workspace = true in:")
    for manifest in missing:
        print(f"  {manifest}")
    sys.exit(1)


if __name__ == "__main__":
    main()
