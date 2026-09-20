#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///
"""Verify every workspace member has a README.

A crate's README is where the next contributor finds how to run it, how to see
it working, and which of its surprises will cost them an afternoon. None of
that is derivable from the source, and nobody notices it is absent until they
need it, which is the worst moment to find out.

    scripts/check-crate-readmes.py

Existence is all this checks. Whether a README is still true is a judgement
nothing here can make. See clients/README.md for the minimal shape and
daemon/README.md for one carrying a runbook.

Exits non-zero and names each crate that is missing one. Run by `just check`
and by the prek hook.
"""

from __future__ import annotations

import json
import pathlib
import subprocess
import sys


def crates_without_readmes() -> list[str]:
    """Complaints about workspace members whose README is absent or empty."""
    try:
        metadata = subprocess.check_output(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"]
        )
    except FileNotFoundError:
        raise SystemExit("cargo is not on PATH; run this from a Rust toolchain shell")
    except subprocess.CalledProcessError as error:
        raise SystemExit(f"cargo metadata failed with exit code {error.returncode}")

    root = pathlib.Path.cwd()
    complaints = []
    for package in json.loads(metadata)["packages"]:
        readme = pathlib.Path(package["manifest_path"]).parent / "README.md"
        shown = readme.relative_to(root) if readme.is_relative_to(root) else readme
        if not readme.is_file():
            complaints.append(f"{shown}: missing")
        elif readme.stat().st_size == 0:
            complaints.append(f"{shown}: empty")
    return complaints


def main() -> None:
    complaints = crates_without_readmes()
    if not complaints:
        return
    print("ERROR: every workspace member needs a README:")
    for complaint in complaints:
        print(f"  {complaint}")
    print()
    print("Cover what the crate is for, how to run it, and the gotchas that are not")
    print("visible in the source. clients/README.md is the minimal shape;")
    print("daemon/README.md adds a runbook.")
    sys.exit(1)


if __name__ == "__main__":
    main()
