# SPDX-License-Identifier: Apache-2.0
"""T1 — the simulated user: the validating bracket, scan cost, determinism."""

import pytest

from fluence_eval.metrics import keystroke_savings_pct
from fluence_eval.sources import LetterByLetter, Oracle, Prediction, PredictionSource
from fluence_eval.user import (
    LETTER_BY_LETTER,
    PREDICTION,
    MotorProfile,
    SimulatedUser,
    SuggestionPolicy,
)

TARGET = "salut ca va"  # 3 words, 11 characters including the 2 spaces
NO_FATIGUE = MotorProfile(dwell_ms=800, fatigue_ms_per_keystroke=0, error_rate=0.0)


class _UselessSource(PredictionSource):
    """Always offers one non-matching word — models the consultation trap."""

    @property
    def name(self) -> str:
        return "useless"

    def predict(self, context: str, word_prefix: str) -> list[Prediction]:
        return [Prediction("zzz")]


def test_letter_by_letter_is_the_zero_savings_floor() -> None:
    user = SimulatedUser(NO_FATIGUE, LETTER_BY_LETTER, seed=0)
    counters = user.type_target(TARGET, LetterByLetter())

    assert counters.keystrokes == counters.baseline_keystrokes == len(TARGET)
    assert counters.suggestions_consulted == 0
    # No scan cost: time is purely the 11 keystrokes' dwell.
    assert counters.elapsed_ms == len(TARGET) * 800


def test_oracle_is_the_high_savings_ceiling() -> None:
    user = SimulatedUser(NO_FATIGUE, PREDICTION, seed=0)
    counters = user.type_target(TARGET, Oracle())

    # One selection per word (3) + 2 typed spaces = 5 keystrokes vs 11.
    assert counters.keystrokes == 5
    assert counters.suggestions_accepted == 3
    assert counters.harmful_consultations == 0
    expected_ks = (len(TARGET) - 5) / len(TARGET) * 100.0
    assert counters.keystrokes < counters.baseline_keystrokes
    assert keystroke_savings_pct(
        counters.keystrokes, counters.baseline_keystrokes
    ) == pytest.approx(expected_ks)


def test_oracle_beats_letter_by_letter_the_validating_bracket() -> None:
    # The harness validates itself: the floor saves nothing, the ceiling saves
    # a lot, and the ceiling strictly dominates the floor (SPEC §8.A).
    floor = SimulatedUser(NO_FATIGUE, LETTER_BY_LETTER, seed=0).type_target(
        TARGET, LetterByLetter()
    )
    ceiling = SimulatedUser(NO_FATIGUE, PREDICTION, seed=0).type_target(TARGET, Oracle())
    assert ceiling.keystrokes < floor.keystrokes


def test_harmful_suggestions_are_pure_cost() -> None:
    # A source that is always wrong saves no keystrokes yet still costs scan
    # time — the « consultation trap » must show up as harmful, slower, KS% 0.
    user = SimulatedUser(NO_FATIGUE, PREDICTION, seed=0)
    counters = user.type_target(TARGET, _UselessSource())

    assert counters.keystrokes == len(TARGET)  # nothing saved
    assert counters.suggestions_accepted == 0
    assert counters.harmful_consultations == counters.suggestions_consulted > 0
    # Strictly slower than letter-by-letter: the scan cost is dead weight.
    floor = SimulatedUser(NO_FATIGUE, LETTER_BY_LETTER, seed=0).type_target(
        TARGET, LetterByLetter()
    )
    assert counters.elapsed_ms > floor.elapsed_ms


def test_scan_cost_is_billed_per_consultation() -> None:
    # Each useless consultation costs base + per-suggestion (350 + 150×1).
    user = SimulatedUser(NO_FATIGUE, PREDICTION, seed=0)
    counters = user.type_target(TARGET, _UselessSource())
    floor_ms = len(TARGET) * 800
    assert counters.elapsed_ms == floor_ms + counters.suggestions_consulted * (350 + 150)


def test_progressive_fatigue_accumulates() -> None:
    profile = MotorProfile(dwell_ms=800, fatigue_ms_per_keystroke=10, error_rate=0.0)
    user = SimulatedUser(profile, LETTER_BY_LETTER, seed=0)
    counters = user.type_target("ab", LetterByLetter())
    # Keystroke 0 costs 800, keystroke 1 costs 810.
    assert counters.elapsed_ms == 800 + 810


def test_counters_are_deterministic_for_a_given_seed() -> None:
    # Same seed ⇒ identical counters, even with motor noise on (the CI gate
    # depends on this reproducibility).
    noisy = MotorProfile(dwell_ms=800, fatigue_ms_per_keystroke=0, error_rate=0.3)
    first = SimulatedUser(noisy, PREDICTION, seed=123).type_target(TARGET, _UselessSource())
    second = SimulatedUser(noisy, PREDICTION, seed=123).type_target(TARGET, _UselessSource())
    assert first == second


def test_consultation_skipped_when_disabled() -> None:
    # consult_every = 0 disables consultation even with a real source.
    policy = SuggestionPolicy(consult_every=0)
    counters = SimulatedUser(NO_FATIGUE, policy, seed=0).type_target(TARGET, Oracle())
    assert counters.suggestions_consulted == 0
    assert counters.keystrokes == len(TARGET)
