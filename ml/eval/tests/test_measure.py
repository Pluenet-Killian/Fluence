# SPDX-License-Identifier: Apache-2.0
"""T1 — the value-gate logic of the rephrase measurement (#31)."""

from __future__ import annotations

from fluence_eval.measure import beats_ngram


def test_beats_ngram_requires_winning_on_both_wpm_and_ks() -> None:
    # Ahead on both → pass.
    assert beats_ngram(rephrase_wpm=18.0, rephrase_ks=20.0, ngram_wpm=16.0, ngram_ks=15.0)
    # Ahead on WPM but behind on KS → fail (ADR-0008 requires both).
    assert not beats_ngram(rephrase_wpm=18.0, rephrase_ks=10.0, ngram_wpm=16.0, ngram_ks=15.0)
    # Ahead on KS but behind on WPM → fail.
    assert not beats_ngram(rephrase_wpm=15.0, rephrase_ks=20.0, ngram_wpm=16.0, ngram_ks=15.0)
    # Ties do not count (strictly greater).
    assert not beats_ngram(rephrase_wpm=16.0, rephrase_ks=15.0, ngram_wpm=16.0, ngram_ks=15.0)
