# SPDX-License-Identifier: Apache-2.0
"""T1 — metrics return exact values on hand-built cases (SPEC §8.A).

Every expected value below is chosen so the arithmetic is exact in IEEE-754,
so equality is asserted with ``==``: the CI regression gate « KS% −2 points »
only means something if the numbers are reproducible to the bit.
"""

import pytest

from fluence_eval.metrics import (
    Counters,
    acceptance_rate,
    aggregate_counters,
    harmful_rate,
    keystroke_savings_pct,
    simulated_wpm,
)


@pytest.mark.parametrize(
    ("keystrokes", "baseline", "expected"),
    [
        (75, 100, 25.0),  # saved a quarter of the keystrokes
        (50, 200, 75.0),
        (100, 100, 0.0),  # letter-by-letter: the floor, zero savings by construction
        (0, 0, 0.0),  # empty target: guarded, no division by zero
        (0, 10, 100.0),  # everything predicted (oracle ceiling)
    ],
)
def test_keystroke_savings_pct_is_exact(keystrokes: int, baseline: int, expected: float) -> None:
    assert keystroke_savings_pct(keystrokes, baseline) == expected


@pytest.mark.parametrize(
    ("characters", "elapsed_ms", "expected"),
    [
        (50, 60_000, 10.0),  # 50 chars = 10 words in 1 min
        (25, 30_000, 10.0),  # same rate, half the time and chars
        (0, 0, 0.0),  # nothing typed: guarded
        (100, 60_000, 20.0),
    ],
)
def test_simulated_wpm_is_exact(characters: int, elapsed_ms: int, expected: float) -> None:
    assert simulated_wpm(characters, elapsed_ms) == expected


def test_acceptance_and_harmful_rates_are_complementary() -> None:
    assert acceptance_rate(3, 4) == 0.75
    assert harmful_rate(3, 4) == 0.25
    # By definition they partition the consultations.
    assert acceptance_rate(3, 4) + harmful_rate(3, 4) == 1.0


def test_rates_guard_against_no_consultations() -> None:
    assert acceptance_rate(0, 0) == 0.0
    assert harmful_rate(0, 0) == 0.0


def test_harmful_consultations_is_consulted_minus_accepted() -> None:
    counters = Counters(suggestions_consulted=10, suggestions_accepted=4)
    assert counters.harmful_consultations == 6


def test_aggregate_is_a_micro_average_not_a_mean_of_percentages() -> None:
    # Two dialogues with very different sizes. Averaging their KS% would give
    # (50 + 0)/2 = 25 %; the correct micro-average pools the keystrokes first.
    short = Counters(characters=2, keystrokes=1, baseline_keystrokes=2)
    long = Counters(characters=100, keystrokes=100, baseline_keystrokes=100)
    total = aggregate_counters([short, long])

    assert total.characters == 102
    assert total.keystrokes == 101
    assert total.baseline_keystrokes == 102
    # Pooled: (102 − 101) / 102 ≈ 0.98 %, nowhere near the 25 % a naive mean gives.
    assert keystroke_savings_pct(total.keystrokes, total.baseline_keystrokes) == pytest.approx(
        1 / 102 * 100.0
    )


def test_aggregate_of_nothing_is_all_zeros() -> None:
    total = aggregate_counters([])
    assert total == Counters()
