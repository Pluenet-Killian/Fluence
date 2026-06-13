# SPDX-License-Identifier: Apache-2.0
"""T1 — pure parts of the live rephrase source / semantic acceptor (#31).

The networked paths (the actual HTTP to the hub and the embedding server) are
exercised by the live measurement; here we test the model-free logic: cosine,
SSE-final parsing, and the acceptor's threshold/short-circuit behaviour against
a stub embedder.
"""

from __future__ import annotations

from fluence_eval.live import (
    EmbeddingAcceptor,
    EmbeddingClient,
    cosine,
    first_suggestion,
)


class _StubEmbedder(EmbeddingClient):
    """An embedder returning fixed vectors, never touching the network."""

    def __init__(self, vectors: dict[str, list[float]]) -> None:
        """Map each text to its vector (no base URL — `embed` is overridden)."""
        self._vectors = vectors

    def embed(self, text: str) -> list[float]:
        """Return the pre-set vector for ``text``."""
        return self._vectors[text]


def test_cosine_identical_orthogonal_and_degenerate() -> None:
    assert abs(cosine([1.0, 2.0, 3.0], [1.0, 2.0, 3.0]) - 1.0) < 1e-9
    assert cosine([1.0, 0.0], [0.0, 1.0]) == 0.0
    assert cosine([0.0, 0.0], [1.0, 1.0]) == 0.0


def test_first_suggestion_extracts_the_final_events_top_text() -> None:
    sse = (
        'event: delta\ndata: {"i":0,"text":"Je "}\n\n'
        'event: final\ndata: {"suggestions":[{"text":"Je veux de l\'eau.","score":0.9}]}\n\n'
    )
    assert first_suggestion(sse) == "Je veux de l'eau."


def test_first_suggestion_is_none_without_a_final_event() -> None:
    assert first_suggestion('event: delta\ndata: {"i":0,"text":"x"}\n\n') is None


def test_first_suggestion_is_none_on_empty_suggestions() -> None:
    assert first_suggestion('event: final\ndata: {"suggestions":[]}\n\n') is None


def test_embedding_acceptor_accepts_above_threshold_and_rejects_below() -> None:
    vectors = {
        "candidate near": [1.0, 0.0],
        "the target": [0.99, 0.01],  # near-parallel → cosine ≈ 1
        "candidate far": [0.0, 1.0],  # orthogonal → cosine ≈ 0
    }
    acceptor = EmbeddingAcceptor(_StubEmbedder(vectors), threshold=0.8)
    assert acceptor.accepts("candidate near", "the target")
    assert not acceptor.accepts("candidate far", "the target")


def test_embedding_acceptor_short_circuits_on_exact_match() -> None:
    # A normalized exact match accepts without embedding (the stub would
    # KeyError if `embed` were called).
    acceptor = EmbeddingAcceptor(_StubEmbedder({}), threshold=0.99)
    assert acceptor.accepts("Je veux de l'eau.", "je veux de l'eau")
