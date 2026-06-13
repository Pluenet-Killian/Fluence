# SPDX-License-Identifier: Apache-2.0
"""T1/T6 — the n-gram source measured through the real crate (SPEC §8.A).

Skips when the ``fluence-ngram`` binary is not built (``cargo build -p
fluence-ngram``); CI builds it so the sanity gate runs there.
"""

from pathlib import Path

import pytest

from fluence_data import build_corpus_v0
from fluence_eval.ngram import NgramServer, NgramSource, locate_ngram_binary, train_on_corpus
from fluence_eval.runner import Mode, letter_by_letter_mode, oracle_mode, run_corpus
from fluence_eval.user import PREDICTION, MotorProfile

PROFILE = MotorProfile(dwell_ms=800)


def _binary_or_skip() -> Path:
    binary = locate_ngram_binary()
    if binary is None:
        pytest.skip("fluence-ngram not built (run: cargo build -p fluence-ngram)")
    return binary


def test_server_trains_and_completes() -> None:
    binary = _binary_or_skip()
    with NgramServer.spawn(binary) as server:
        server.train("voudrais voudrais voiture")
        assert "voudrais" in server.complete("vou", 5)
        assert server.complete("zzz", 5) == []


def test_ngram_lands_between_the_floor_and_the_ceiling() -> None:
    # The validating bracket (SPEC §8.A): the real n-gram must beat
    # letter-by-letter and stay under the oracle. This is the sanity gate that
    # proves both the harness and the fallback are wired correctly.
    binary = _binary_or_skip()
    corpus = build_corpus_v0()
    with NgramServer.spawn(binary) as server:
        train_on_corpus(server, corpus)
        mode = Mode("ngram", lambda: NgramSource(server), PREDICTION)
        ngram = run_corpus(corpus, mode, profile=PROFILE, seed=0, suite="test")

    floor = run_corpus(corpus, letter_by_letter_mode(), profile=PROFILE, seed=0, suite="test")
    ceiling = run_corpus(corpus, oracle_mode(), profile=PROFILE, seed=0, suite="test")

    assert floor.aggregate.ks_pct == 0.0
    assert ngram.aggregate.ks_pct > floor.aggregate.ks_pct, "n-gram must beat letter-by-letter"
    assert ngram.aggregate.ks_pct < ceiling.aggregate.ks_pct, "n-gram must stay under the oracle"
