# SPDX-License-Identifier: Apache-2.0
"""The simulated user — the methodological core of the harness (SPEC §8.A).

A user types a target sentence under a motor profile and a suggestion policy,
producing the integer :class:`~fluence_eval.metrics.Counters` the metrics read.
The model, after Cai et al.:

* **motor** — one keystroke per character, each costing a dwell (with optional
  progressive fatigue); spatial slips draw from the AZERTY confusion model.
* **suggestion policy** — consult every *k* characters, paying the billed scan
  cost (a base plus a per-suggestion-read term, SPEC §8.A: 350 ms + 150 ms);
  accept a suggestion iff it matches the word being typed, completing it in one
  keystroke.

A "mode" (letter-by-letter / +prediction / …) is just a (source, policy) pair:
letter-by-letter never consults, +prediction consults a real source.

Determinism: timing is pure integer arithmetic; the only nondeterminism is the
motor-noise RNG, seeded once per user, so a run reproduces to the bit.

v0 scope: the policy accepts on **lexical** match (the suggested word equals the
target word). Semantic acceptance (embeddings) lands with rephrase/replies in
Phase 4 (ADR-0006); intra-word noise affects only the current prediction, words
being assumed corrected once committed.
"""

from __future__ import annotations

import random
from dataclasses import dataclass

from fluence_data import sample_keypress
from fluence_eval.metrics import Counters
from fluence_eval.sources import PredictionSource


@dataclass(frozen=True)
class MotorProfile:
    """Per-selection timing and motor-noise parameters (SPEC §8.A)."""

    #: Base time per keystroke or selection, milliseconds (dwell 600–1500).
    dwell_ms: int = 800
    #: Linear dwell increase per keystroke — optional progressive fatigue.
    fatigue_ms_per_keystroke: int = 0
    #: Spatial slip probability in ``[0, 1]`` (AZERTY confusion; 0–15 %).
    error_rate: float = 0.0


@dataclass(frozen=True)
class SuggestionPolicy:
    """When the user consults suggestions and what each look costs (SPEC §8.A)."""

    #: Consult every *k* characters; ``0`` disables consultation entirely
    #: (the letter-by-letter mode).
    consult_every: int = 0
    #: Suggestions read per consultation (UI guidance: 3 max, SPEC §7.A).
    n_suggestions: int = 3
    #: Fixed cost of one consultation, milliseconds (SPEC §8.A: 350 ms).
    scan_base_ms: int = 350
    #: Added cost per suggestion read, milliseconds (SPEC §8.A: 150 ms).
    scan_per_suggestion_ms: int = 150


#: The letter-by-letter mode: never consult (the KS% baseline policy).
LETTER_BY_LETTER = SuggestionPolicy(consult_every=0)
#: A plain +prediction mode: consult every other character.
PREDICTION = SuggestionPolicy(consult_every=2)


class SimulatedUser:
    """Types targets under a motor profile and a suggestion policy."""

    def __init__(self, profile: MotorProfile, policy: SuggestionPolicy, seed: int) -> None:
        """Build a user. ``seed`` fixes the motor-noise RNG for reproducibility."""
        self._profile = profile
        self._policy = policy
        self._rng = random.Random(seed)
        self._keystroke_index = 0

    def _dwell(self) -> int:
        """Cost of the next keystroke, with progressive fatigue applied."""
        fatigue = self._profile.fatigue_ms_per_keystroke * self._keystroke_index
        self._keystroke_index += 1
        return self._profile.dwell_ms + fatigue

    def type_target(self, target: str, source: PredictionSource) -> Counters:
        """Type ``target`` under the policy, returning the run's counters.

        The baseline is ``len(target)`` keystrokes (one per character,
        letter-by-letter); the policy can only lower the actual keystrokes,
        never raise them — accepting a word costs one selection instead of its
        remaining characters.

        Args:
            target: The sentence the user intends to produce.
            source: The prediction source consulted under the policy.

        Returns:
            The integer counters for this target.
        """
        keystrokes = 0
        elapsed_ms = 0
        offered = 0
        consulted = 0
        accepted = 0

        words = target.split(" ")
        committed: list[str] = []
        for word_index, word in enumerate(words):
            source.begin_word(word)
            context = " ".join(committed)
            typed_chars: list[str] = []

            while len(typed_chars) < len(word):
                if self._should_consult(len(typed_chars)):
                    shown = source.predict(context, "".join(typed_chars))[
                        : self._policy.n_suggestions
                    ]
                    if shown:
                        consulted += 1
                        offered += len(shown)
                        elapsed_ms += self._scan_cost(len(shown))
                        if any(prediction.text == word for prediction in shown):
                            accepted += 1
                            keystrokes += 1  # one selection completes the word
                            elapsed_ms += self._dwell()
                            break
                # Press the next character (a spatial slip is possible but does
                # not change the keystroke count — only the prefix seen next).
                typed_chars.append(
                    sample_keypress(word[len(typed_chars)], self._profile.error_rate, self._rng)
                )
                keystrokes += 1
                elapsed_ms += self._dwell()

            # The word is committed as intended (accepted suggestion, or typed
            # and assumed corrected before moving on — v0 simplification).
            committed.append(word)

            if word_index < len(words) - 1:  # the separating space
                keystrokes += 1
                elapsed_ms += self._dwell()

        return Counters(
            characters=len(target),
            keystrokes=keystrokes,
            baseline_keystrokes=len(target),
            elapsed_ms=elapsed_ms,
            suggestions_offered=offered,
            suggestions_consulted=consulted,
            suggestions_accepted=accepted,
        )

    def _should_consult(self, chars_typed: int) -> bool:
        """Whether to consult after ``chars_typed`` characters of the word."""
        if self._policy.consult_every <= 0:
            return False
        return chars_typed % self._policy.consult_every == 0

    def _scan_cost(self, shown: int) -> int:
        """Billed cost of one consultation showing ``shown`` suggestions."""
        return self._policy.scan_base_ms + self._policy.scan_per_suggestion_ms * shown
