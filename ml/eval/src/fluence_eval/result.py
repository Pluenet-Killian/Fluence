# SPDX-License-Identifier: Apache-2.0
"""Versioned evaluation-result format (SPEC §8.A): per-dialogue and aggregate.

A :class:`MetricBundle` stores the integer counters as its only state; the
rates (KS%, WPM, acceptance, harmful) are :func:`pydantic.computed_field`
properties derived from them, so the serialised result is both human-readable
*and* impossible to make internally inconsistent. An :class:`EvalReport`
bundles one source × one mode over a suite, with a micro-averaged aggregate.

The format is versioned (``schema_version``); the nightly performance page and
the CI regression gate read it, so a breaking change increments the version.
"""

from __future__ import annotations

from pydantic import BaseModel, ConfigDict, Field, computed_field

from fluence_eval.metrics import (
    Counters,
    acceptance_rate,
    aggregate_counters,
    harmful_rate,
    keystroke_savings_pct,
    simulated_wpm,
)

#: Result schema version. Bumped on any backward-incompatible change.
RESULT_SCHEMA_VERSION = 1


class MetricBundle(BaseModel):
    """Integer counters for one run, with derived rates as computed fields.

    ``extra="ignore"`` (not ``forbid``) so a serialised bundle round-trips: its
    JSON carries the computed rates, which are recomputed — not re-ingested —
    on load. The required counter fields still reject a mistyped key (the real
    field would then be missing).
    """

    model_config = ConfigDict(frozen=True, extra="ignore")

    characters: int = Field(ge=0)
    keystrokes: int = Field(ge=0)
    baseline_keystrokes: int = Field(ge=0)
    elapsed_ms: int = Field(ge=0)
    suggestions_offered: int = Field(ge=0)
    suggestions_consulted: int = Field(ge=0)
    suggestions_accepted: int = Field(ge=0)

    @classmethod
    def from_counters(cls, counters: Counters) -> MetricBundle:
        """Build a bundle from raw integer counters (the canonical constructor)."""
        return cls(
            characters=counters.characters,
            keystrokes=counters.keystrokes,
            baseline_keystrokes=counters.baseline_keystrokes,
            elapsed_ms=counters.elapsed_ms,
            suggestions_offered=counters.suggestions_offered,
            suggestions_consulted=counters.suggestions_consulted,
            suggestions_accepted=counters.suggestions_accepted,
        )

    # mypy does not support a decorator stacked on @property (prop-decorator),
    # and the pydantic plugin does not lift this. The property bodies are fully
    # typed, so each ignore suppresses only the known tool gap, never a real
    # type error.
    @computed_field  # type: ignore[prop-decorator]
    @property
    def ks_pct(self) -> float:
        """Keystroke savings vs letter-by-letter, in percent."""
        return keystroke_savings_pct(self.keystrokes, self.baseline_keystrokes)

    @computed_field  # type: ignore[prop-decorator]
    @property
    def wpm(self) -> float:
        """Simulated words per minute."""
        return simulated_wpm(self.characters, self.elapsed_ms)

    @computed_field  # type: ignore[prop-decorator]
    @property
    def acceptance_rate(self) -> float:
        """Fraction of consultations that ended in an acceptance."""
        return acceptance_rate(self.suggestions_accepted, self.suggestions_consulted)

    @computed_field  # type: ignore[prop-decorator]
    @property
    def harmful_rate(self) -> float:
        """Fraction of consultations that were pure cost."""
        return harmful_rate(self.suggestions_accepted, self.suggestions_consulted)


class DialogueResult(BaseModel):
    """The metrics of one source × one mode on one dialogue."""

    model_config = ConfigDict(frozen=True, extra="forbid")

    dialogue_id: str = Field(min_length=1)
    source: str = Field(min_length=1)
    mode: str = Field(min_length=1)
    metrics: MetricBundle


class EvalReport(BaseModel):
    """A full run: one source × one mode over a suite, with its aggregate."""

    model_config = ConfigDict(frozen=True, extra="forbid")

    schema_version: int = RESULT_SCHEMA_VERSION
    suite: str = Field(min_length=1)
    seed: int
    source: str = Field(min_length=1)
    mode: str = Field(min_length=1)
    per_dialogue: list[DialogueResult]
    aggregate: MetricBundle

    @classmethod
    def from_results(
        cls,
        *,
        suite: str,
        seed: int,
        source: str,
        mode: str,
        results: list[DialogueResult],
    ) -> EvalReport:
        """Assemble a report and compute its micro-averaged aggregate."""
        aggregate = MetricBundle.from_counters(
            aggregate_counters(
                Counters(
                    characters=result.metrics.characters,
                    keystrokes=result.metrics.keystrokes,
                    baseline_keystrokes=result.metrics.baseline_keystrokes,
                    elapsed_ms=result.metrics.elapsed_ms,
                    suggestions_offered=result.metrics.suggestions_offered,
                    suggestions_consulted=result.metrics.suggestions_consulted,
                    suggestions_accepted=result.metrics.suggestions_accepted,
                )
                for result in results
            )
        )
        return cls(
            suite=suite,
            seed=seed,
            source=source,
            mode=mode,
            per_dialogue=results,
            aggregate=aggregate,
        )
