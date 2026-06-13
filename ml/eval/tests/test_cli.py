# SPDX-License-Identifier: Apache-2.0
"""T1/T6 — the eval CLI: the run-suite bracket and the KS% regression gate."""

import argparse
from pathlib import Path

import pytest

from fluence_data import build_corpus_v0
from fluence_eval.cli import cmd_check, run_suite
from fluence_eval.metrics import Counters
from fluence_eval.ngram import locate_ngram_binary
from fluence_eval.result import DialogueResult, EvalReport, MetricBundle


def _report_with_keystrokes(path: Path, keystrokes: int) -> Path:
    """Write a one-dialogue ngram report whose baseline is 100 keystrokes."""
    bundle = MetricBundle.from_counters(
        Counters(characters=100, keystrokes=keystrokes, baseline_keystrokes=100, elapsed_ms=1000)
    )
    report = EvalReport.from_results(
        suite="t",
        seed=0,
        source="ngram",
        mode="ngram",
        results=[DialogueResult(dialogue_id="d", source="ngram", mode="ngram", metrics=bundle)],
    )
    path.write_text(report.model_dump_json(), encoding="utf-8")
    return path


def test_run_suite_brackets_the_ngram() -> None:
    if locate_ngram_binary() is None:
        pytest.skip("fluence-ngram not built (run: cargo build -p fluence-ngram)")
    reports = run_suite(suite="test", corpus=build_corpus_v0(), seed=0)
    floor = reports["letter_by_letter"].aggregate.ks_pct
    ngram = reports["ngram"].aggregate.ks_pct
    ceiling = reports["oracle"].aggregate.ks_pct
    assert floor == 0.0
    assert floor < ngram < ceiling


def test_check_tolerates_a_small_drop_and_fails_a_large_one(tmp_path: Path) -> None:
    # Baseline KS% = 50 (50/100 keystrokes saved).
    baseline = _report_with_keystrokes(tmp_path / "baseline.json", 50)

    # Candidate KS% = 49 (a 1-point drop) is within the 2-point budget.
    small = _report_with_keystrokes(tmp_path / "small.json", 51)
    ok = cmd_check(argparse.Namespace(baseline=baseline, candidate=small, max_regression=2.0))
    assert ok == 0

    # Candidate KS% = 40 (a 10-point drop) trips the gate.
    large = _report_with_keystrokes(tmp_path / "large.json", 60)
    failed = cmd_check(argparse.Namespace(baseline=baseline, candidate=large, max_regression=2.0))
    assert failed == 1


def test_committed_baseline_is_not_regressed_by_a_fresh_run(tmp_path: Path) -> None:
    # The committed baseline must still hold against a fresh n-gram run — the
    # gate the CI enforces. Skips without the binary.
    if locate_ngram_binary() is None:
        pytest.skip("fluence-ngram not built")
    baseline_path = Path(__file__).parents[1] / "baselines" / "ngram-pr-v0.json"
    reports = run_suite(suite="pr", corpus=build_corpus_v0(), seed=20260613)
    candidate = tmp_path / "ngram.json"
    candidate.write_text(reports["ngram"].model_dump_json(), encoding="utf-8")
    result = cmd_check(
        argparse.Namespace(baseline=baseline_path, candidate=candidate, max_regression=2.0)
    )
    assert result == 0
