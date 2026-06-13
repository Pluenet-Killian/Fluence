# SPDX-License-Identifier: Apache-2.0
"""Evaluation metrics (SPEC §8.A): KS%, simulated WPM, acceptance, harmful rate.

The harness keeps its accounting in **integer counters** (:class:`Counters`) —
keystrokes, characters, milliseconds, consultation tallies. Only the final
ratios are floats. This keeps results bit-identical across platforms (no
transcendental functions on the path, no float accumulation order to depend
on), which is what makes the CI regression gate « KS% −2 points » trustworthy.

All functions are pure and total: a degenerate denominator yields ``0.0``
rather than raising, so an empty or trivial dialogue never crashes a suite.
"""

from __future__ import annotations

from collections.abc import Iterable
from dataclasses import dataclass

#: Characters per "word" for WPM, the standard AAC/typing convention.
CHARS_PER_WORD = 5
#: Milliseconds per minute (WPM denominator).
MS_PER_MINUTE = 60_000


@dataclass(frozen=True)
class Counters:
    """Integer accounting for one run (one dialogue, or an aggregate).

    Every field is an exact tally produced by the simulated user; the derived
    rates are computed from these and never stored independently.
    """

    #: Characters actually produced (the target text length).
    characters: int = 0
    #: Keystrokes the simulated user spent under the evaluated policy.
    keystrokes: int = 0
    #: Keystrokes a pure letter-by-letter user would have spent (the baseline).
    baseline_keystrokes: int = 0
    #: Simulated elapsed time, milliseconds (motor dwell + billed scan cost).
    elapsed_ms: int = 0
    #: Suggestions shown across all consultations (informational).
    suggestions_offered: int = 0
    #: Times the user scanned the suggestion list (each paid the scan cost).
    suggestions_consulted: int = 0
    #: Consultations that ended in accepting a suggestion (``≤ consulted``).
    suggestions_accepted: int = 0

    @property
    def harmful_consultations(self) -> int:
        """Consultations that accepted nothing — pure cost (SPEC §8.A)."""
        return self.suggestions_consulted - self.suggestions_accepted


def keystroke_savings_pct(keystrokes: int, baseline_keystrokes: int) -> float:
    """Keystroke savings vs letter-by-letter, in percent (SPEC §8.A).

    ``KS% = (baseline − keystrokes) / baseline × 100``. A letter-by-letter
    run spends exactly the baseline, so its KS% is ``0.0`` by construction —
    the floor of the validating bracket.

    Args:
        keystrokes: Keystrokes spent under the evaluated policy.
        baseline_keystrokes: Letter-by-letter keystroke count.

    Returns:
        The savings percentage, or ``0.0`` if the baseline is empty.
    """
    if baseline_keystrokes <= 0:
        return 0.0
    return (baseline_keystrokes - keystrokes) / baseline_keystrokes * 100.0


def simulated_wpm(characters: int, elapsed_ms: int) -> float:
    """Words per minute from the temporal model (SPEC §8.A).

    A "word" is :data:`CHARS_PER_WORD` characters. Rearranged to a single
    division (``characters × MS_PER_MINUTE / (CHARS_PER_WORD × elapsed_ms)``)
    to keep one rounding step only.

    Args:
        characters: Characters produced.
        elapsed_ms: Simulated elapsed time in milliseconds.

    Returns:
        Simulated WPM, or ``0.0`` if no time elapsed.
    """
    if elapsed_ms <= 0:
        return 0.0
    return characters * MS_PER_MINUTE / (CHARS_PER_WORD * elapsed_ms)


def acceptance_rate(suggestions_accepted: int, suggestions_consulted: int) -> float:
    """Fraction of consultations that ended in an acceptance.

    Args:
        suggestions_accepted: Consultations that accepted a suggestion.
        suggestions_consulted: Total consultations.

    Returns:
        Acceptance rate in ``[0, 1]``, or ``0.0`` if nothing was consulted.
    """
    if suggestions_consulted <= 0:
        return 0.0
    return suggestions_accepted / suggestions_consulted


def harmful_rate(suggestions_accepted: int, suggestions_consulted: int) -> float:
    """Fraction of consultations that were pure cost (consulted, accepted nothing).

    The complement of :func:`acceptance_rate`; reported separately because the
    "consultation-cost trap" is a first-class failure mode (SPEC §8.A).

    Args:
        suggestions_accepted: Consultations that accepted a suggestion.
        suggestions_consulted: Total consultations.

    Returns:
        Harmful-consultation rate in ``[0, 1]``, or ``0.0`` if nothing was consulted.
    """
    if suggestions_consulted <= 0:
        return 0.0
    return (suggestions_consulted - suggestions_accepted) / suggestions_consulted


def aggregate_counters(counters: Iterable[Counters]) -> Counters:
    """Sum counters field-by-field (micro-average — sum tallies, then derive).

    Aggregating the *counters* and deriving rates once is correct; averaging
    per-dialogue percentages would over-weight short dialogues.

    Args:
        counters: The per-run counters to combine.

    Returns:
        A single :class:`Counters` with each field summed.
    """
    total = Counters()
    for item in counters:
        total = Counters(
            characters=total.characters + item.characters,
            keystrokes=total.keystrokes + item.keystrokes,
            baseline_keystrokes=total.baseline_keystrokes + item.baseline_keystrokes,
            elapsed_ms=total.elapsed_ms + item.elapsed_ms,
            suggestions_offered=total.suggestions_offered + item.suggestions_offered,
            suggestions_consulted=total.suggestions_consulted + item.suggestions_consulted,
            suggestions_accepted=total.suggestions_accepted + item.suggestions_accepted,
        )
    return total
