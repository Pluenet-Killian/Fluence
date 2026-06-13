# SPDX-License-Identifier: Apache-2.0
"""Live (networked) rephrase source and semantic acceptor for the value run (#31).

These plug the real engine into the model-free framework of
:mod:`fluence_eval.rephrase`:

* :class:`HubRephraseSource` POSTs to the hub's real ``/suggest`` (mode
  ``rephrase``) and returns its top suggestion — so the value run measures the
  production prompt (``fluence-accel``) and model, not a reimplementation;
* :class:`EmbeddingAcceptor` accepts a candidate when its embedding cosine with
  the target clears a threshold — the **semantic** acceptance a good rephrase
  needs (it may change words), via a local embedding model served by
  ``llama-server`` (``/v1/embeddings``).

Only stdlib ``urllib`` is used (loopback HTTP); the pure parts — cosine,
SSE-final parsing, threshold logic — are unit tested here, and the networked
paths are exercised by the live measurement.
"""

from __future__ import annotations

import json
import math
import urllib.request
from collections.abc import Sequence

from fluence_eval.rephrase import Acceptor, SentenceSource, normalize

#: Default acceptance threshold for bge-m3 cosine. Equivalent French sentences
#: score well above this, unrelated ones well below; tune per embedding model
#: and calibrate on a labelled sample before trusting a value number.
DEFAULT_ACCEPT_THRESHOLD = 0.80


def cosine(a: Sequence[float], b: Sequence[float]) -> float:
    """Cosine similarity of two equal-length vectors (``0.0`` if degenerate)."""
    dot = sum(x * y for x, y in zip(a, b, strict=True))
    norm_a = math.sqrt(sum(x * x for x in a))
    norm_b = math.sqrt(sum(y * y for y in b))
    if norm_a == 0.0 or norm_b == 0.0:
        return 0.0
    return dot / (norm_a * norm_b)


def _post_json(url: str, payload: dict[str, object], timeout: float, token: str | None) -> str:
    """POST ``payload`` as JSON to ``url`` and return the response body text."""
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"content-type": "application/json"},
    )
    if token is not None:
        request.add_header("X-Fluence-Token", token)
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return str(response.read().decode("utf-8"))


def first_suggestion(sse_body: str) -> str | None:
    """The first suggestion text of an SSE ``/suggest`` response (or ``None``).

    Scans ``event: final`` frames and returns ``suggestions[0].text``. Tolerates
    the interleaved ``delta`` frames and blank separators.
    """
    for frame in sse_body.split("\n\n"):
        event = ""
        data = ""
        for line in frame.splitlines():
            if line.startswith("event:"):
                event = line[len("event:") :].strip()
            elif line.startswith("data:"):
                data = line[len("data:") :].strip()
        if event == "final" and data:
            suggestions = json.loads(data).get("suggestions") or []
            if suggestions:
                text = suggestions[0].get("text")
                return text if isinstance(text, str) else None
    return None


class EmbeddingClient:
    """Embeds text via a ``llama-server`` ``/v1/embeddings`` endpoint."""

    def __init__(self, base_url: str, *, timeout: float = 30.0) -> None:
        """Target the embedding server at ``base_url`` (scheme + host + port)."""
        self._url = base_url.rstrip("/") + "/v1/embeddings"
        self._timeout = timeout

    def embed(self, text: str) -> list[float]:
        """Return the embedding vector of ``text``."""
        raw = _post_json(self._url, {"input": text}, self._timeout, None)
        vector = json.loads(raw)["data"][0]["embedding"]
        return [float(component) for component in vector]


class EmbeddingAcceptor(Acceptor):
    """Accepts a candidate when its embedding cosine with the target clears a threshold."""

    def __init__(
        self, client: EmbeddingClient, *, threshold: float = DEFAULT_ACCEPT_THRESHOLD
    ) -> None:
        """Use ``client`` to embed, accepting at cosine ≥ ``threshold``."""
        self._client = client
        self._threshold = threshold

    def accepts(self, candidate: str, target: str) -> bool:
        """Accept on a normalized exact match, else on embedding cosine."""
        if normalize(candidate) == normalize(target):
            return True
        similarity = cosine(self._client.embed(candidate), self._client.embed(target))
        return similarity >= self._threshold


class HubRephraseSource(SentenceSource):
    """Drives the hub's real ``/suggest`` (mode ``rephrase``) over HTTP + SSE."""

    def __init__(
        self,
        base_url: str,
        token: str,
        *,
        session: str = "eval",
        timeout: float = 120.0,
    ) -> None:
        """Target the hub at ``base_url`` with a ``control``-scoped ``token``."""
        self._url = base_url.rstrip("/") + f"/api/v1/sessions/{session}/suggest"
        self._token = token
        self._timeout = timeout

    @property
    def name(self) -> str:
        """Source name (recorded in the report)."""
        return "hub-rephrase"

    def rephrase(self, context: str, fragment: str) -> str | None:
        """Return the hub's top rephrase suggestion for ``fragment`` (or ``None``)."""
        payload: dict[str, object] = {
            "mode": "rephrase",
            "draft": fragment,
            "n": 3,
            "slot": "main",
        }
        return first_suggestion(_post_json(self._url, payload, self._timeout, self._token))
