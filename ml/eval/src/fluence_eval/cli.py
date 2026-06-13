# SPDX-License-Identifier: Apache-2.0
"""Offline evaluation CLI (SPEC §8.A, PLAN 3.6) — ``python -m fluence_eval``.

``run`` types the corpus under each mode (letter-by-letter, oracle, and the
n-gram when its binary is built) and writes one :class:`EvalReport` per mode.
``check`` fails the build when a candidate's KS% regresses past a threshold
versus a committed baseline — the « régression KS% > 2 points = échec » gate.

The numbers are deterministic (integer counters, fixed seed), so a baseline is
a stable golden: regenerate it intentionally when KS% genuinely improves.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from fluence_data import Dialogue, build_corpus_v0, load_jsonl
from fluence_eval.ngram import NgramServer, NgramSource, locate_ngram_binary, train_on_corpus
from fluence_eval.result import EvalReport
from fluence_eval.runner import Mode, letter_by_letter_mode, oracle_mode, run_corpus
from fluence_eval.user import PREDICTION, MotorProfile

#: Reference motor profile for the offline suites (a mid-range dwell).
DEFAULT_PROFILE = MotorProfile(dwell_ms=800)
#: Fixed seed so suites are reproducible to the bit.
DEFAULT_SEED = 20260613


def _load_corpus(path: Path | None) -> list[Dialogue]:
    """Load the corpus from JSONL, or the built-in seed when no path is given."""
    return load_jsonl(path) if path is not None else build_corpus_v0()


def run_suite(*, suite: str, corpus: list[Dialogue], seed: int) -> dict[str, EvalReport]:
    """Run every available mode over ``corpus`` and return their reports.

    The n-gram mode is included only when its binary is built; the always-on
    baselines (letter-by-letter, oracle) bracket it.
    """
    reports: dict[str, EvalReport] = {
        "letter_by_letter": run_corpus(
            corpus, letter_by_letter_mode(), profile=DEFAULT_PROFILE, seed=seed, suite=suite
        ),
        "oracle": run_corpus(
            corpus, oracle_mode(), profile=DEFAULT_PROFILE, seed=seed, suite=suite
        ),
    }
    binary = locate_ngram_binary()
    if binary is not None:
        with NgramServer.spawn(binary) as server:
            train_on_corpus(server, corpus)
            mode = Mode("ngram", lambda: NgramSource(server), PREDICTION)
            reports["ngram"] = run_corpus(
                corpus, mode, profile=DEFAULT_PROFILE, seed=seed, suite=suite
            )
    return reports


def _summary(reports: dict[str, EvalReport]) -> str:
    """A small fixed-width table of the headline metrics per mode."""
    header = f"{'mode':<18}{'KS%':>8}{'WPM':>8}{'accept':>8}"
    rows = [
        f"{name:<18}{r.aggregate.ks_pct:>8.2f}{r.aggregate.wpm:>8.2f}{r.aggregate.acceptance_rate:>8.2f}"
        for name, r in reports.items()
    ]
    return "\n".join([header, *rows])


def cmd_run(args: argparse.Namespace) -> int:
    """Run a suite, optionally write the reports, and print the summary."""
    corpus = _load_corpus(args.corpus)
    reports = run_suite(suite=args.suite, corpus=corpus, seed=args.seed)
    out: Path | None = args.out
    if out is not None:
        out.mkdir(parents=True, exist_ok=True)
        for name, report in reports.items():
            (out / f"{name}.json").write_text(report.model_dump_json(indent=2), encoding="utf-8")
    print(_summary(reports))
    return 0


def cmd_check(args: argparse.Namespace) -> int:
    """Fail (exit 1) if the candidate's KS% regressed past the threshold."""
    baseline = EvalReport.model_validate_json(Path(args.baseline).read_text(encoding="utf-8"))
    candidate = EvalReport.model_validate_json(Path(args.candidate).read_text(encoding="utf-8"))
    regression = baseline.aggregate.ks_pct - candidate.aggregate.ks_pct
    print(
        f"KS%: baseline {baseline.aggregate.ks_pct:.2f} → candidate "
        f"{candidate.aggregate.ks_pct:.2f} (Δ {-regression:+.2f} points)"
    )
    if regression > args.max_regression:
        print(
            f"FAIL: KS% regressed by {regression:.2f} > {args.max_regression:.2f} "
            "points (SPEC §8.A CI gate)",
            file=sys.stderr,
        )
        return 1
    return 0


def main(argv: list[str] | None = None) -> int:
    """Parse arguments and dispatch to the chosen subcommand."""
    parser = argparse.ArgumentParser(prog="fluence_eval", description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    run_parser = sub.add_parser("run", help="run a suite over the corpus")
    run_parser.add_argument("--suite", default="pr", help="suite label recorded in the reports")
    run_parser.add_argument(
        "--corpus", type=Path, default=None, help="JSONL corpus (default: seed v0)"
    )
    run_parser.add_argument(
        "--out", type=Path, default=None, help="directory to write per-mode reports"
    )
    run_parser.add_argument("--seed", type=int, default=DEFAULT_SEED, help="motor-noise seed")
    run_parser.set_defaults(func=cmd_run)

    check_parser = sub.add_parser("check", help="fail if KS% regressed vs a baseline")
    check_parser.add_argument("--baseline", type=Path, required=True, help="baseline report JSON")
    check_parser.add_argument("--candidate", type=Path, required=True, help="candidate report JSON")
    check_parser.add_argument(
        "--max-regression", type=float, default=2.0, help="allowed KS% drop, points"
    )
    check_parser.set_defaults(func=cmd_check)

    args = parser.parse_args(argv)
    result: int = args.func(args)
    return result


if __name__ == "__main__":
    sys.exit(main())
