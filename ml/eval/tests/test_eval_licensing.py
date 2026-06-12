# SPDX-License-Identifier: Apache-2.0
"""Phase 0 plumbing test: pytest reaches this package (PLAN task 0.3)."""

from pathlib import Path

import tomllib


def test_package_license_follows_d_10_1() -> None:
    """D-10.1: ml/ packages are reusable bricks, licensed Apache-2.0."""
    pyproject = Path(__file__).parents[1] / "pyproject.toml"
    with pyproject.open("rb") as fh:
        manifest = tomllib.load(fh)
    assert manifest["project"]["license"] == "Apache-2.0"
