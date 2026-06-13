# SPDX-License-Identifier: Apache-2.0
"""Prediction sources — what the simulated user consults (SPEC §8.A).

A source maps the text typed so far to ranked word completions, mirroring the
hub's acceleration engine (`fluence-protocol`'s ``Suggestion`` / next-chars).
Three live in Phase 3, and together they *bracket* every real engine:

* :class:`LetterByLetter` — predicts nothing; the KS% floor (0 by definition,
  the mandatory letter-by-letter baseline of §8.A).
* :class:`Oracle` — knows the word being typed and offers it immediately; the
  KS% ceiling. It cheats *by design*, through the explicit :meth:`begin_word`
  channel — an honest source ignores it.
* the n-gram (PLAN 3.5, a separate slice) — a real fallback that must land
  strictly between the two.
"""

from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass


@dataclass(frozen=True)
class Prediction:
    """One predicted completion of the current word."""

    #: The full suggested word (the user accepts it iff it equals the target).
    text: str


class PredictionSource(ABC):
    """A source of word completions for the current input."""

    @property
    @abstractmethod
    def name(self) -> str:
        """Stable identifier used in reports and baselines."""

    @abstractmethod
    def predict(self, context: str, word_prefix: str) -> list[Prediction]:
        """Rank completions of ``word_prefix`` given the preceding ``context``.

        Args:
            context: Text committed before the current word (prior words).
            word_prefix: Characters of the current word typed so far.

        Returns:
            Candidate full words, best first (may be empty).
        """

    def begin_word(self, target_word: str) -> None:  # noqa: B027
        """Hook called before the user types each word.

        Intentionally a concrete no-op, not an abstract method: honest sources
        ignore it, only the :class:`Oracle` overrides it to learn the word being
        typed (the cheating upper-bound baseline by design). Making it abstract
        would force every source to implement an empty body.
        """


class LetterByLetter(PredictionSource):
    """Predicts nothing — the letter-by-letter baseline (KS% floor)."""

    @property
    def name(self) -> str:
        """Source name."""
        return "letter_by_letter"

    def predict(self, context: str, word_prefix: str) -> list[Prediction]:
        """Always empty: every character is typed."""
        return []


class Oracle(PredictionSource):
    """Knows the target word and offers it at once — the KS% ceiling."""

    def __init__(self) -> None:
        """Start with no word primed."""
        self._word: str | None = None

    @property
    def name(self) -> str:
        """Source name."""
        return "oracle"

    def begin_word(self, target_word: str) -> None:
        """Learn the word the user is about to type (the cheat)."""
        self._word = target_word

    def predict(self, context: str, word_prefix: str) -> list[Prediction]:
        """Offer the primed word once it is non-empty, else nothing."""
        if self._word:
            return [Prediction(self._word)]
        return []
