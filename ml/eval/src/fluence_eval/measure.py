# SPDX-License-Identifier: Apache-2.0
r"""Live value measurement (#31, PLAN Phase 4 T6): does rephrase beat the n-gram?

Runs three modes on the *same* corpus and the same KS% definition, then compares:

* **letter-by-letter** — the floor (0 % by construction);
* **n-gram** — the mandatory baseline (the real ``fluence-ngram`` crate);
* **rephrase** — via the **real hub** ``/suggest`` (mode ``rephrase``, so the
  production ``fluence-accel`` prompt and model are measured), accepted
  *semantically* by embedding cosine (:class:`~fluence_eval.live.EmbeddingAcceptor`).

The gate (amended, ADR-0008): rephrase beats the n-gram on **WPM** (primary —
the ×3 star metric, SPEC §1.2) **and** on out-of-domain **KS%** (the n-gram is
trained on train+dev and evaluated on the held-out test split). This is a local
/ nightly measurement — it needs a running hub (a capable model behind it) and
an embedding server; CI gating waits for a self-hosted reference machine (PLAN
§0/§7). No threshold is adjusted to make it pass.

Usage (servers already up)::

    python -m fluence_eval.measure --hub-url http://127.0.0.1:7411 \\
        --embed-url http://127.0.0.1:8090 --data-dir <hub data dir>
"""

from __future__ import annotations

import argparse
import json
import sys
import urllib.request
from pathlib import Path

from fluence_data import Dialogue, Split, VariantKind, build_corpus_v0, load_jsonl
from fluence_eval.live import (
    DEFAULT_ACCEPT_THRESHOLD,
    EmbeddingAcceptor,
    EmbeddingClient,
    HubRephraseSource,
)
from fluence_eval.ngram import NgramServer, NgramSource, locate_ngram_binary, train_on_corpus
from fluence_eval.rephrase import evaluate_rephrase
from fluence_eval.result import EvalReport
from fluence_eval.runner import Mode, letter_by_letter_mode, run_corpus
from fluence_eval.user import PREDICTION, MotorProfile

#: Reference motor profile (matches the offline suites).
PROFILE = MotorProfile(dwell_ms=800)
#: Fixed seed for the word-level baselines (reproducibility).
SEED = 20260613


def beats_ngram(rephrase_wpm: float, rephrase_ks: float, ngram_wpm: float, ngram_ks: float) -> bool:
    """The amended #31 value gate (ADR-0008).

    Rephrase must beat the n-gram on **WPM** (primary — the ×3 star metric,
    SPEC §1.2) **and** on **KS%** (here measured out-of-domain). Both strictly
    greater; the old in-domain ``+10 KS points`` proxy is dropped — it was capped
    by fragment length and inflated by an overfit n-gram.
    """
    return rephrase_wpm > ngram_wpm and rephrase_ks > ngram_ks


def _post(url: str, payload: dict[str, object], token: str | None, timeout: float) -> str:
    """POST JSON and return the body text (small helper for pairing)."""
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"content-type": "application/json"},
    )
    if token is not None:
        request.add_header("X-Fluence-Token", token)
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return str(response.read().decode("utf-8"))


def pair_control_token(hub_url: str, system_token: str, *, timeout: float = 30.0) -> str:
    """Open a pairing window with the system token and pair a ``control`` device."""
    window = _post(hub_url + "/api/v1/pair/window", {"scope": "control"}, system_token, timeout)
    code = json.loads(window)["code"]
    paired = _post(
        hub_url + "/pair",
        {"code": code, "device_name": "eval", "device_kind": "cli"},
        None,
        timeout,
    )
    return str(json.loads(paired)["device_token"])


def _ngram_report(train: list[Dialogue], test: list[Dialogue], suite: str) -> EvalReport | None:
    """Run the real n-gram baseline, or ``None`` when its binary is not built.

    Trains on ``train`` (train+dev) and evaluates on ``test`` — never on its own
    training data — so the baseline is honest (out-of-domain), not the inflated
    in-domain number of a tiny corpus the model has memorised (ADR-0008).
    """
    binary = locate_ngram_binary()
    if binary is None:
        return None
    with NgramServer.spawn(binary) as server:
        train_on_corpus(server, train)
        mode = Mode("ngram", lambda: NgramSource(server), PREDICTION)
        return run_corpus(test, mode, profile=PROFILE, seed=SEED, suite=suite)


def _summary(reports: dict[str, EvalReport]) -> str:
    """A small fixed-width table of KS% / WPM / acceptance per mode."""
    header = f"{'mode':<16}{'KS%':>8}{'WPM':>8}{'accept':>8}"
    rows = [
        f"{name:<16}{r.aggregate.ks_pct:>8.2f}{r.aggregate.wpm:>8.2f}"
        f"{r.aggregate.acceptance_rate:>8.2f}"
        for name, r in reports.items()
    ]
    return "\n".join([header, *rows])


def cmd_measure(args: argparse.Namespace) -> int:
    """Run the three modes on the held-out test split and apply the value gate."""
    corpus = load_jsonl(args.corpus) if args.corpus is not None else build_corpus_v0()
    train = [d for d in corpus if d.split in (Split.TRAIN, Split.DEV)]
    test = [d for d in corpus if d.split == Split.TEST]
    if not test:
        # No frozen split: fall back to the whole corpus, but then the n-gram is
        # overfit in-domain and its KS% is inflated — say so (ADR-0008).
        print(
            "warning: corpus has no test split; measuring in-domain "
            "(the n-gram is overfit, its KS% is inflated)",
            file=sys.stderr,
        )
        train, test = corpus, corpus

    token: str = args.token or pair_control_token(
        args.hub_url, (args.data_dir / "system.token").read_text(encoding="utf-8").strip()
    )

    rephrase = evaluate_rephrase(
        test,
        HubRephraseSource(args.hub_url, token),
        EmbeddingAcceptor(EmbeddingClient(args.embed_url), threshold=args.threshold),
        suite="value",
        variant_kind=VariantKind(args.variant),
        profile=PROFILE,
    )
    reports: dict[str, EvalReport] = {
        "letter_by_letter": run_corpus(
            test, letter_by_letter_mode(), profile=PROFILE, seed=SEED, suite="value"
        ),
        "rephrase": rephrase,
    }
    ngram = _ngram_report(train, test, "value")
    if ngram is not None:
        reports = {
            "letter_by_letter": reports["letter_by_letter"],
            "ngram": ngram,
            "rephrase": rephrase,
        }

    print(_summary(reports))
    if args.out is not None:
        args.out.mkdir(parents=True, exist_ok=True)
        for name, report in reports.items():
            (args.out / f"{name}.json").write_text(
                report.model_dump_json(indent=2), encoding="utf-8"
            )

    if ngram is None:
        print("\nn-gram binary not built — cannot apply the value gate.", file=sys.stderr)
        return 0
    r, n = rephrase.aggregate, ngram.aggregate
    verdict = "PASS" if beats_ngram(r.wpm, r.ks_pct, n.wpm, n.ks_pct) else "FAIL"
    print(
        f"\nvalue gate (#31, ADR-0008): rephrase vs n-gram | "
        f"WPM {r.wpm:.2f} vs {n.wpm:.2f} ({r.wpm - n.wpm:+.2f}), "
        f"KS% {r.ks_pct:.2f} vs {n.ks_pct:.2f} ({r.ks_pct - n.ks_pct:+.2f}) -> {verdict}"
    )
    return 0 if verdict == "PASS" else 1


def main(argv: list[str] | None = None) -> int:
    """Parse arguments and run the value measurement."""
    parser = argparse.ArgumentParser(prog="fluence_eval.measure", description=__doc__)
    parser.add_argument("--hub-url", default="http://127.0.0.1:7411", help="hub base URL")
    parser.add_argument(
        "--embed-url", default="http://127.0.0.1:8090", help="embedding server base URL"
    )
    parser.add_argument(
        "--data-dir", type=Path, default=Path(".fluence"), help="hub data dir (for system.token)"
    )
    parser.add_argument("--token", default=None, help="control token (skips pairing if given)")
    parser.add_argument("--corpus", type=Path, default=None, help="JSONL corpus (default: seed v0)")
    parser.add_argument("--out", type=Path, default=None, help="directory to write reports")
    parser.add_argument(
        "--threshold",
        type=float,
        default=DEFAULT_ACCEPT_THRESHOLD,
        help="embedding cosine acceptance threshold",
    )
    parser.add_argument(
        "--variant", default="telegraphic", help="input variant used as the typed fragment"
    )
    parser.set_defaults(func=cmd_measure)
    args = parser.parse_args(argv)
    result: int = args.func(args)
    return result


if __name__ == "__main__":
    sys.exit(main())
