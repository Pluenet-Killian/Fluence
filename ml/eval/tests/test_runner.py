# SPDX-License-Identifier: Apache-2.0
"""T1 — the runner reports the validating bracket over a corpus (SPEC §8.A)."""

from fluence_data import Dialogue, Register, Situation, Speaker, Split, Turn
from fluence_eval.result import EvalReport
from fluence_eval.runner import letter_by_letter_mode, oracle_mode, run_corpus
from fluence_eval.user import MotorProfile

PROFILE = MotorProfile(dwell_ms=800)


def _corpus() -> list[Dialogue]:
    return [
        Dialogue(
            id="d-001",
            situation=Situation.REPAS,
            register=Register.FAMILIER,
            split=Split.DEV,
            turns=[
                Turn(speaker=Speaker.PARTNER, text="tu veux quoi"),
                Turn(speaker=Speaker.USER, text="je voudrais des pates"),
            ],
        ),
        Dialogue(
            id="d-002",
            situation=Situation.SOINS,
            register=Register.NEUTRE,
            split=Split.DEV,
            turns=[
                Turn(speaker=Speaker.USER, text="merci beaucoup"),
            ],
        ),
    ]


def test_letter_by_letter_report_has_zero_savings() -> None:
    report = run_corpus(_corpus(), letter_by_letter_mode(), profile=PROFILE, seed=0, suite="test")
    assert report.mode == "letter_by_letter"
    assert report.source == "letter_by_letter"
    assert report.suite == "test"
    assert len(report.per_dialogue) == 2
    assert report.aggregate.ks_pct == 0.0


def test_oracle_report_beats_the_floor() -> None:
    floor = run_corpus(_corpus(), letter_by_letter_mode(), profile=PROFILE, seed=0, suite="test")
    ceiling = run_corpus(_corpus(), oracle_mode(), profile=PROFILE, seed=0, suite="test")
    assert ceiling.aggregate.ks_pct > floor.aggregate.ks_pct
    assert ceiling.aggregate.ks_pct > 0.0
    # Every dialogue is measured; the oracle accepts in both.
    assert len(ceiling.per_dialogue) == 2
    assert ceiling.aggregate.suggestions_accepted > 0


def test_report_round_trips_through_json() -> None:
    report = run_corpus(_corpus(), oracle_mode(), profile=PROFILE, seed=0, suite="test")
    assert EvalReport.model_validate_json(report.model_dump_json()) == report


def test_empty_corpus_yields_an_empty_report() -> None:
    report = run_corpus([], letter_by_letter_mode(), profile=PROFILE, seed=0, suite="test")
    assert report.per_dialogue == []
    assert report.aggregate.ks_pct == 0.0
