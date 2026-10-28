#!/usr/bin/env python3
"""Checks that versatiles/Cargo.toml carries the workspace's release profile.

Cargo reads profiles only from the root manifest of a build, so the copy in
versatiles/Cargo.toml is what `cargo install versatiles` gets, and the one in
the root Cargo.toml is what everything built in the workspace gets (#291). This
fails when they differ.

Usage: python3 scripts/check-release-profile.py
"""

import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def release_profile(manifest: str) -> dict:
    with open(ROOT / manifest, "rb") as file:
        return tomllib.load(file).get("profile", {}).get("release", {})


def flatten(table: dict, prefix: str = "") -> dict:
    """`{"package": {"a": {"x": 1}}}` as `{"package.a.x": 1}`, for the report."""
    flat = {}
    for key, value in table.items():
        if isinstance(value, dict):
            flat.update(flatten(value, f"{prefix}{key}."))
        else:
            flat[f"{prefix}{key}"] = value
    return flat


workspace = flatten(release_profile("Cargo.toml"))
crate = flatten(release_profile("versatiles/Cargo.toml"))

if workspace == crate:
    print("versatiles/Cargo.toml has the workspace's release profile")
    sys.exit(0)

print("versatiles/Cargo.toml differs from the workspace's release profile:")
for key in sorted(workspace.keys() | crate.keys()):
    if workspace.get(key) != crate.get(key):
        print(f"  profile.release.{key}: workspace {workspace.get(key)!r}, versatiles {crate.get(key)!r}")
sys.exit(1)
