#!/usr/bin/env python3
"""Verify all workspace members opt into workspace lints via [lints] workspace = true."""
import json
import pathlib
import subprocess
import sys

meta = json.loads(
    subprocess.check_output(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"]
    )
)
missing = [
    p["manifest_path"]
    for p in meta["packages"]
    if "workspace = true" not in pathlib.Path(p["manifest_path"]).read_text()
]
if missing:
    print("ERROR: missing [lints]\\n  workspace = true in:")
    for f in missing:
        print(f"  {f}")
    sys.exit(1)
