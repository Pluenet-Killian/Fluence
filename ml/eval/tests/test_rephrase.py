# SPDX-License-Identifier: Apache-2.0
"""T1 — sentence-level rephrase evaluation (PLAN Phase 4 T6, issue #31)."""

from __future__ import annotations

from fluence_data import (
    Dialogue,
    InputVariant,
    Register,
    Situation,
    Speaker,
    Split,
    Turn,
    VariantKind,
)
from fluence_eval.rephrase import (
    ExactAcceptor,
    SentenceSource,
    evaluate_rephrase,
    normalize,
    score_turn,
)
from fluence_eval.user import MotorProfile

PROFILE = MotorProfile(dwell_ms=800)
TARGET = "je voudrais de l'eau fraîche"
FRAGMENT = "eau fraiche"


class StubRephrase(SentenceSource):
    """A deterministic source returning a fixed fragment→sentence mapping."""

    def __init__(self, mapping: dict[str, str]) -> None:
        """Store the fixed mapping."""
        self._mapping = mapping

    @property
    def name(self) -> str:
        """Source name."""
        return "stub"

    def rephrase(self, context: str, fragment: str) -> str | None:
        """Return the mapped sentence, or ``None`` for an unknown fragment."""
        return self._mapping.get(fragment)


def _dialogue() -> Dialogue:
    return Dialogue(
        id="d-rephrase",
        situation=Situation.REPAS,
        register=Register.FAMILIER,
        split=Split.TEST,
        turns=[
            Turn(speaker=Speaker.PARTNER, text="tu veux quelque chose"),
            Turn(
                speaker=Speaker.USER,
                text=TARGET,
                variants=[InputVariant(kind=VariantKind.TELEGRAPHIC, text=FRAGMENT)],
            ),
        ],
    )


def test_normalize_is_case_space_and_punctuation_insensitive() -> None:
    assert normalize("  Je VEUX  de l'eau. ") == "je veux de l'eau"


def test_exact_acceptor_accepts_normalized_equal_and_rejects_otherwise() -> None:
    acceptor = ExactAcceptor()
    assert acceptor.accepts("Je veux de l'eau.", "je veux de l'eau")
    assert not acceptor.accepts("autre chose", "je veux de l'eau")


def test_accepted_rephrase_saves_keystrokes() -> None:
    counters = score_turn(
        StubRephrase({FRAGMENT: TARGET}),
        ExactAcceptor(),
        context="",
        target=TARGET,
        fragment=FRAGMENT,
        dwell_ms=800,
    )
    # Only the fragment was typed; the rephrase was accepted.
    assert counters.keystrokes == len(FRAGMENT)
    assert counters.baseline_keystrokes == len(TARGET)
    assert counters.suggestions_accepted == 1
    assert counters.harmful_consultations == 0


def test_rejected_rephrase_falls_back_to_full_typing() -> None:
    counters = score_turn(
        StubRephrase({FRAGMENT: "phrase sans rapport"}),
        ExactAcceptor(),
        context="",
        target=TARGET,
        fragment=FRAGMENT,
        dwell_ms=800,
    )
    # No saving, and the look was a harmful (billed, fruitless) consultation.
    assert counters.keystrokes == len(TARGET)
    assert counters.suggestions_accepted == 0
    assert counters.harmful_consultations == 1


def test_missing_fragment_is_plain_letter_by_letter() -> None:
    counters = score_turn(
        StubRephrase({}),
        ExactAcceptor(),
        context="",
        target=TARGET,
        fragment=None,
        dwell_ms=800,
    )
    assert counters.keystrokes == len(TARGET)
    assert counters.suggestions_consulted == 0


def test_evaluate_rephrase_reports_positive_savings_when_accepted() -> None:
    report = evaluate_rephrase(
        [_dialogue()],
        StubRephrase({FRAGMENT: TARGET}),
        ExactAcceptor(),
        suite="test",
        profile=PROFILE,
    )
    assert report.mode == "rephrase:stub"
    assert report.source == "stub"
    assert len(report.per_dialogue) == 1
    assert report.aggregate.ks_pct > 0.0
    assert report.aggregate.suggestions_accepted == 1


def test_evaluate_rephrase_saves_nothing_when_rejected() -> None:
    report = evaluate_rephrase(
        [_dialogue()],
        StubRephrase({FRAGMENT: "phrase sans rapport"}),
        ExactAcceptor(),
        suite="test",
        profile=PROFILE,
    )
    assert report.aggregate.ks_pct == 0.0
    assert report.aggregate.suggestions_consulted == 1
    assert report.aggregate.suggestions_accepted == 0
