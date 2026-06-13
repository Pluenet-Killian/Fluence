# SPDX-License-Identifier: Apache-2.0
"""T1 — the result format derives rates from counters and aggregates correctly."""

from fluence_eval.metrics import Counters
from fluence_eval.result import (
    RESULT_SCHEMA_VERSION,
    DialogueResult,
    EvalReport,
    MetricBundle,
)


def test_metric_bundle_derives_rates_from_counters() -> None:
    bundle = MetricBundle.from_counters(
        Counters(
            characters=50,
            keystrokes=75,
            baseline_keystrokes=100,
            elapsed_ms=60_000,
            suggestions_consulted=4,
            suggestions_accepted=3,
        )
    )
    assert bundle.ks_pct == 25.0
    assert bundle.wpm == 10.0
    assert bundle.acceptance_rate == 0.75
    assert bundle.harmful_rate == 0.25


def test_computed_rates_appear_in_serialised_json() -> None:
    # The nightly performance page and the CI gate read the JSON; the derived
    # rates must be present, not just the raw counters.
    bundle = MetricBundle.from_counters(
        Counters(characters=50, keystrokes=75, baseline_keystrokes=100, elapsed_ms=60_000)
    )
    dumped = bundle.model_dump()
    assert dumped["ks_pct"] == 25.0
    assert dumped["wpm"] == 10.0
    assert set(dumped) >= {"keystrokes", "baseline_keystrokes", "ks_pct", "wpm"}


def test_eval_report_aggregate_is_micro_averaged() -> None:
    results = [
        DialogueResult(
            dialogue_id="d-001",
            source="oracle",
            mode="prediction",
            metrics=MetricBundle.from_counters(
                Counters(characters=2, keystrokes=1, baseline_keystrokes=2, elapsed_ms=1_000)
            ),
        ),
        DialogueResult(
            dialogue_id="d-002",
            source="oracle",
            mode="prediction",
            metrics=MetricBundle.from_counters(
                Counters(characters=100, keystrokes=100, baseline_keystrokes=100, elapsed_ms=10_000)
            ),
        ),
    ]
    report = EvalReport.from_results(
        suite="pr", seed=42, source="oracle", mode="prediction", results=results
    )
    assert report.schema_version == RESULT_SCHEMA_VERSION
    assert report.aggregate.characters == 102
    assert report.aggregate.keystrokes == 101
    assert report.aggregate.baseline_keystrokes == 102
    assert report.aggregate.elapsed_ms == 11_000


def test_eval_report_round_trips_through_json() -> None:
    report = EvalReport.from_results(
        suite="pr",
        seed=7,
        source="letter_by_letter",
        mode="prediction",
        results=[
            DialogueResult(
                dialogue_id="d-001",
                source="letter_by_letter",
                mode="prediction",
                metrics=MetricBundle.from_counters(
                    Counters(characters=10, keystrokes=10, baseline_keystrokes=10, elapsed_ms=8_000)
                ),
            )
        ],
    )
    restored = EvalReport.model_validate_json(report.model_dump_json())
    assert restored == report
