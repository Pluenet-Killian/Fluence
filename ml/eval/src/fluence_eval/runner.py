# SPDX-License-Identifier: Apache-2.0
"""Run a mode over a corpus → an :class:`EvalReport` (SPEC §8.A).

A :class:`Mode` bundles what fully defines one acceleration policy to measure:
a name, a factory for its prediction source, and the user's suggestion policy.
:func:`run_corpus` types every user turn of every dialogue under that mode and
returns the per-dialogue results plus their micro-averaged aggregate.

Each dialogue gets a fresh source and a fresh user (seeded identically), so a
dialogue's result is independent of corpus order — partial re-runs and the
per-dialogue delta the CI comments are stable.
"""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass

from fluence_data import Dialogue
from fluence_eval.metrics import aggregate_counters
from fluence_eval.result import DialogueResult, EvalReport, MetricBundle
from fluence_eval.sources import LetterByLetter, Oracle, PredictionSource
from fluence_eval.user import (
    LETTER_BY_LETTER,
    PREDICTION,
    MotorProfile,
    SimulatedUser,
    SuggestionPolicy,
)


@dataclass(frozen=True)
class Mode:
    """One acceleration policy to measure: a name, a source, a usage policy."""

    name: str
    source_factory: Callable[[], PredictionSource]
    policy: SuggestionPolicy


def letter_by_letter_mode() -> Mode:
    """The mandatory baseline: predict nothing, never consult (KS% floor)."""
    return Mode("letter_by_letter", LetterByLetter, LETTER_BY_LETTER)


def oracle_mode() -> Mode:
    """The cheating upper bound: the oracle source under a +prediction policy."""
    return Mode("oracle", Oracle, PREDICTION)


def run_corpus(
    dialogues: list[Dialogue],
    mode: Mode,
    *,
    profile: MotorProfile,
    seed: int,
    suite: str,
) -> EvalReport:
    """Type every dialogue under ``mode`` and report per-dialogue + aggregate.

    Args:
        dialogues: The corpus (already filtered to the desired split).
        mode: The acceleration policy to measure.
        profile: The simulated user's motor profile.
        seed: Fixes the motor-noise RNG (reproducibility).
        suite: Suite label recorded in the report (e.g. ``"pr"``).

    Returns:
        The report for this mode over the corpus.
    """
    results: list[DialogueResult] = []
    for dialogue in dialogues:
        source = mode.source_factory()
        user = SimulatedUser(profile, mode.policy, seed)
        per_turn = [user.type_target(turn.text, source) for turn in dialogue.user_turns]
        aggregated = aggregate_counters(per_turn)
        results.append(
            DialogueResult(
                dialogue_id=dialogue.id,
                source=source.name,
                mode=mode.name,
                metrics=MetricBundle.from_counters(aggregated),
            )
        )
    return EvalReport.from_results(
        suite=suite,
        seed=seed,
        source=mode.source_factory().name,
        mode=mode.name,
        results=results,
    )
