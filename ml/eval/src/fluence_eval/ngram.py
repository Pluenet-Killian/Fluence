# SPDX-License-Identifier: Apache-2.0
"""The n-gram prediction source — drives the real ``fluence-ngram`` crate.

The harness measures the *actual* fallback model (SPEC §8.A, ADR-0006), so this
talks to the Rust crate over its JSON-lines `serve` protocol rather than
reimplementing it in Python. One long-lived subprocess holds the trained model
for the whole run; :class:`NgramSource` issues one request per consultation.

The binary is produced by ``cargo build -p fluence-ngram``; :func:`locate_ngram_binary`
finds it (``FLUENCE_NGRAM_BIN`` overrides). When it is absent — a checkout that
has not built the crate — callers skip the n-gram (the rest of the harness is
pure Python and unaffected).
"""

from __future__ import annotations

import json
import os
import subprocess
from pathlib import Path
from types import TracebackType

from fluence_data import Dialogue
from fluence_eval.sources import Prediction, PredictionSource


def locate_ngram_binary() -> Path | None:
    """Find the built ``fluence-ngram`` binary, or ``None`` if not built.

    Honours ``FLUENCE_NGRAM_BIN``; otherwise looks under the workspace
    ``target/{release,debug}``.
    """
    override = os.environ.get("FLUENCE_NGRAM_BIN")
    if override:
        candidate = Path(override)
        return candidate if candidate.is_file() else None
    repo_root = Path(__file__).resolve().parents[4]
    name = "fluence-ngram.exe" if os.name == "nt" else "fluence-ngram"
    for profile in ("release", "debug"):
        candidate = repo_root / "target" / profile / name
        if candidate.is_file():
            return candidate
    return None


class NgramServer:
    """A long-lived ``fluence-ngram serve`` subprocess (one model in memory)."""

    def __init__(self, process: subprocess.Popen[str]) -> None:
        """Wrap an already-spawned server process."""
        self._process = process

    @classmethod
    def spawn(cls, binary: Path) -> NgramServer:
        """Start the server binary with piped stdin/stdout (UTF-8, line-buffered)."""
        process = subprocess.Popen(
            [str(binary)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            bufsize=1,
        )
        return cls(process)

    def _rpc(self, request: dict[str, object]) -> dict[str, object]:
        """Send one request line and read one response line."""
        if self._process.stdin is None or self._process.stdout is None:
            msg = "ngram server has no pipes"
            raise RuntimeError(msg)
        self._process.stdin.write(json.dumps(request) + "\n")
        self._process.stdin.flush()
        line = self._process.stdout.readline()
        if not line:
            msg = "ngram server closed the connection"
            raise RuntimeError(msg)
        response: dict[str, object] = json.loads(line)
        if "error" in response:
            msg = f"ngram server error: {response['error']}"
            raise RuntimeError(msg)
        return response

    def train(self, text: str) -> None:
        """Count the words of ``text`` into the server's model."""
        self._rpc({"train": {"text": text}})

    def complete(self, prefix: str, n: int) -> list[str]:
        """Return up to ``n`` completions of ``prefix``, best first."""
        response = self._rpc({"complete": {"prefix": prefix, "n": n}})
        words = response["words"]
        if not isinstance(words, list):
            msg = "ngram server returned a malformed completion list"
            raise RuntimeError(msg)
        return [str(word) for word in words]

    def close(self) -> None:
        """Close stdin and wait for the server to exit."""
        if self._process.stdin is not None:
            self._process.stdin.close()
        self._process.wait(timeout=5)

    def __enter__(self) -> NgramServer:
        """Enter the context manager."""
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        """Shut the server down on exit."""
        self.close()


def train_on_corpus(server: NgramServer, dialogues: list[Dialogue]) -> None:
    """Train the server's model on every turn's text (the model's vocabulary)."""
    for dialogue in dialogues:
        for turn in dialogue.turns:
            server.train(turn.text)


class NgramSource(PredictionSource):
    """Prediction source backed by the real ``fluence-ngram`` model."""

    def __init__(self, server: NgramServer, *, candidates: int = 5) -> None:
        """Wrap a (trained) server; ``candidates`` caps completions per query."""
        self._server = server
        self._candidates = candidates

    @property
    def name(self) -> str:
        """Source name (the mandatory n-gram baseline)."""
        return "ngram"

    def predict(self, context: str, word_prefix: str) -> list[Prediction]:
        """Word completions of ``word_prefix`` from the model (v0 ignores context)."""
        words = self._server.complete(word_prefix, self._candidates)
        return [Prediction(word) for word in words]
