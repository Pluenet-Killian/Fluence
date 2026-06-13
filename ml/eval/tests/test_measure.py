# SPDX-License-Identifier: Apache-2.0
"""T1 — the value-gate logic of the rephrase measurement (#31)."""

from __future__ import annotations

from fluence_eval.measure import GATE_POINTS, gate_passes, value_delta_points


def test_value_delta_points_is_signed() -> None:
    assert value_delta_points(50.0, 35.0) == 15.0
    assert value_delta_points(30.0, 35.0) == -5.0


def test_gate_passes_only_at_or_above_the_ten_point_margin() -> None:
    assert gate_passes(45.0, 35.0)  # exactly +10 → pass
    assert gate_passes(60.0, 35.0)  # well above → pass
    assert not gate_passes(44.99, 35.0)  # just under +10 → fail
    assert not gate_passes(35.0, 35.0)  # tie → fail


def test_gate_margin_is_ten_points() -> None:
    assert GATE_POINTS == 10.0
