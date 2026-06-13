# SPDX-License-Identifier: Apache-2.0
"""Sentence-level rephrase evaluation (PLAN Phase 4 T6, issue #31).

Word prediction (:mod:`fluence_eval.user`) measures completing each word as it
is typed. **Rephrase** is a different acceleration: the user types a short,
noisy *telegraphic* fragment of the whole sentence, the engine returns a full
corrected sentence, and the user accepts it iff it conveys the intended meaning
— a **semantic** judgement, not a lexical one (a good rephrase changes words).

This module measures that flow so it stays comparable, on the same corpus and
the same KS% definition, to the word-level n-gram and letter-by-letter baselines
(the §8.A bracket). Per user turn:

* a :class:`SentenceSource` maps ``(context, fragment)`` → a candidate sentence —
  the real one drives the hub's ``/suggest`` (reusing the ``fluence-accel``
  prompt, not reimplementing it); a deterministic stub tests the mechanics;
* an :class:`Acceptor` decides candidate-vs-target acceptance — exact match for
  deterministic tests, embedding cosine for the real value run;
* on acceptance the sentence costs only the fragment's keystrokes
  (``KS% = (len(target) − len(fragment)) / len(target)``); on rejection — or no
  candidate, or no fragment — the user falls back to typing the whole target
  letter by letter, so that turn saves nothing.

The semantic acceptor and the hub-driven source live in follow-up slices; this
module is the deterministic, model-free framework they plug into.
"""

from __future__ import annotations

from abc import ABC, abstractmethod

from fluence_data import Dialogue, Turn, VariantKind
from fluence_eval.metrics import Counters, aggregate_counters
from fluence_eval.result import DialogueResult, EvalReport, MetricBundle
from fluence_eval.user import MotorProfile

#: Billed cost of one rephrase consultation, milliseconds (SPEC §8.A: a 350 ms
#: base plus 150 ms per suggestion read; a rephrase surfaces one full sentence).
SCAN_BASE_MS = 350
SCAN_PER_SUGGESTION_MS = 150

#: Input-variant kinds that stand in for "the noisy fragment the user typed".
FRAGMENT_KINDS = (VariantKind.TELEGRAPHIC, VariantKind.NOISED, VariantKind.ABBREVIATED)


class SentenceSource(ABC):
    """Rephrases a noisy fragment into a full candidate sentence."""

    @property
    @abstractmethod
    def name(self) -> str:
        """Stable identifier recorded in reports."""

    @abstractmethod
    def rephrase(self, context: str, fragment: str) -> str | None:
        """Return a candidate sentence for ``fragment`` (``None`` if it cannot).

        Args:
            context: Prior conversation turns, most recent last (may be empty).
            fragment: The noisy telegraphic text the user typed.

        Returns:
            A full corrected sentence, or ``None`` when the source declines.
        """


class Acceptor(ABC):
    """Decides whether a candidate sentence conveys the target's meaning."""

    @abstractmethod
    def accepts(self, candidate: str, target: str) -> bool:
        """Whether ``candidate`` is an acceptable rephrase of ``target``."""


def normalize(text: str) -> str:
    """Lowercase, collapse whitespace, strip surrounding punctuation."""
    return " ".join(text.lower().split()).strip(" .!?…,;:")


class ExactAcceptor(Acceptor):
    """Accepts on normalized exact match — deterministic, no model (tests/CI).

    Lexical, so it under-counts good rephrases that legitimately change words;
    the real value run uses an embedding acceptor. This exists to exercise the
    harness mechanics and as a conservative floor.
    """

    def accepts(self, candidate: str, target: str) -> bool:
        """Normalized-equality acceptance."""
        return normalize(candidate) == normalize(target)


def fragment_of(turn: Turn, kind: VariantKind) -> str | None:
    """The text of ``turn``'s input variant of ``kind`` (``None`` if absent)."""
    for variant in turn.variants:
        if variant.kind == kind:
            return variant.text
    return None


def _letter_by_letter(target: str, dwell_ms: int) -> Counters:
    """Counters for typing ``target`` in full, letter by letter (no saving)."""
    length = len(target)
    return Counters(
        characters=length,
        keystrokes=length,
        baseline_keystrokes=length,
        elapsed_ms=length * dwell_ms,
    )


def score_turn(
    source: SentenceSource,
    acceptor: Acceptor,
    context: str,
    target: str,
    fragment: str | None,
    dwell_ms: int,
) -> Counters:
    """Counters for one user turn under the rephrase policy.

    With no usable fragment (or no candidate) the user simply types the target.
    Otherwise they type the fragment and consult once: an accepted candidate
    keeps only the fragment's keystrokes; a rejected one falls back to full
    typing, and the consultation is then a billed *harmful* look.
    """
    if not fragment:
        return _letter_by_letter(target, dwell_ms)

    candidate = source.rephrase(context, fragment)
    if candidate is None:
        return _letter_by_letter(target, dwell_ms)

    baseline = len(target)
    scan = SCAN_BASE_MS + SCAN_PER_SUGGESTION_MS
    if acceptor.accepts(candidate, target):
        keystrokes = len(fragment)
        return Counters(
            characters=baseline,
            keystrokes=keystrokes,
            baseline_keystrokes=baseline,
            elapsed_ms=keystrokes * dwell_ms + scan,
            suggestions_offered=1,
            suggestions_consulted=1,
            suggestions_accepted=1,
        )
    # Rejected: the target is typed in full; the look was pure cost (harmful).
    return Counters(
        characters=baseline,
        keystrokes=baseline,
        baseline_keystrokes=baseline,
        elapsed_ms=baseline * dwell_ms + scan,
        suggestions_offered=1,
        suggestions_consulted=1,
        suggestions_accepted=0,
    )


def evaluate_rephrase(
    dialogues: list[Dialogue],
    source: SentenceSource,
    acceptor: Acceptor,
    *,
    suite: str,
    variant_kind: VariantKind = VariantKind.TELEGRAPHIC,
    profile: MotorProfile | None = None,
    seed: int = 0,
) -> EvalReport:
    """Measure ``source`` rephrasing each user turn's noisy fragment.

    KS% here uses the same definition and per-turn target as the word-level
    baselines on the same corpus, so the value gate « rephrase beats the n-gram
    by ≥ 10 points of KS% » (PLAN Phase 4 T6, issue #31) is a direct comparison.

    Args:
        dialogues: The corpus split to measure.
        source: The rephrase source (the real hub, or a test stub).
        acceptor: Candidate-vs-target acceptance (exact, or embedding cosine).
        suite: Suite label recorded in the report.
        variant_kind: Which input variant stands in for the typed fragment.
        profile: Motor profile (only its dwell is used here); default applied.
        seed: Recorded for reproducibility; the flow is deterministic given a
            deterministic source and acceptor.

    Returns:
        The report for this rephrase source over the corpus.
    """
    dwell_ms = (profile or MotorProfile()).dwell_ms
    mode = f"rephrase:{source.name}"
    results: list[DialogueResult] = []
    for dialogue in dialogues:
        committed: list[str] = []
        per_turn: list[Counters] = []
        for turn in dialogue.user_turns:
            context = " ".join(committed)
            per_turn.append(
                score_turn(
                    source,
                    acceptor,
                    context,
                    turn.text,
                    fragment_of(turn, variant_kind),
                    dwell_ms,
                )
            )
            committed.append(turn.text)
        results.append(
            DialogueResult(
                dialogue_id=dialogue.id,
                source=source.name,
                mode=mode,
                metrics=MetricBundle.from_counters(aggregate_counters(per_turn)),
            )
        )
    return EvalReport.from_results(
        suite=suite, seed=seed, source=source.name, mode=mode, results=results
    )
