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
from fluence_eval.result import (
    RESULT_SCHEMA_VERSION,
    DialogueResult,
    EvalReport,
    MetricBundle,
)

__all__ = [
    "CHARS_PER_WORD",
    "RESULT_SCHEMA_VERSION",
    "Counters",
    "DialogueResult",
    "EvalReport",
    "MetricBundle",
    "acceptance_rate",
    "aggregate_counters",
    "harmful_rate",
    "keystroke_savings_pct",
    "simulated_wpm",
]
