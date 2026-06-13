# SPDX-License-Identifier: Apache-2.0
"""Simulation harness — the project's compass (SPEC §8.A, D-8.1).

Implements the simulated-user model (motor noise via spatial confusion
matrices, dwell timing, billed suggestion-scan cost), the metrics (KS%,
simulated WPM, acceptance rate, harmful-suggestion rate), mandatory baselines
and ablations, and the CI gates (KS% regression > 2 points fails the build).
Runs offline per PR and end-to-end against the real hub on reference machines.
Same harness, public subset: FluenceBench-FR (D-8.3).

Phase 3 (« la boussole ») builds it incrementally: PLAN task 3.1 (this
module's formats and metrics), 3.2 (simulated user), 3.5 (n-gram source),
3.6 (CI wiring).
"""

from fluence_eval.metrics import (
    CHARS_PER_WORD,
    Counters,
    acceptance_rate,
    aggregate_counters,
    harmful_rate,
    keystroke_savings_pct,
    simulated_wpm,
)
from fluence_eval.ngram import (
    NgramServer,
    NgramSource,
    locate_ngram_binary,
    train_on_corpus,
)
from fluence_eval.result import (
    RESULT_SCHEMA_VERSION,
    DialogueResult,
    EvalReport,
    MetricBundle,
)
from fluence_eval.runner import (
    Mode,
    letter_by_letter_mode,
    oracle_mode,
    run_corpus,
)
from fluence_eval.sources import (
    LetterByLetter,
    Oracle,
    Prediction,
    PredictionSource,
)
from fluence_eval.user import (
    LETTER_BY_LETTER,
    PREDICTION,
    MotorProfile,
    SimulatedUser,
    SuggestionPolicy,
)

__all__ = [
    "CHARS_PER_WORD",
    "LETTER_BY_LETTER",
    "PREDICTION",
    "RESULT_SCHEMA_VERSION",
    "Counters",
    "DialogueResult",
    "EvalReport",
    "LetterByLetter",
    "MetricBundle",
    "Mode",
    "MotorProfile",
    "NgramServer",
    "NgramSource",
    "Oracle",
    "Prediction",
    "PredictionSource",
    "SimulatedUser",
    "SuggestionPolicy",
    "acceptance_rate",
    "aggregate_counters",
    "harmful_rate",
    "keystroke_savings_pct",
    "letter_by_letter_mode",
    "locate_ngram_binary",
    "oracle_mode",
    "run_corpus",
    "simulated_wpm",
    "train_on_corpus",
]
