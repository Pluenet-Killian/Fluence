# SPDX-License-Identifier: Apache-2.0
r"""CLI: generate the teacher corpus tranche and write it as versioned JSONL (#18).

This is the operational entry point around :mod:`fluence_data.teacher`. It wires
the real HTTP clients — a teacher LLM (``llama-server`` ``/v1/chat/completions``,
e.g. Gemma 4 E4B) and an embedding server (``/v1/embeddings``, e.g. bge-m3) — and
writes the finalized corpus plus a human **review file** (the ≥ 10 % anti-pathos
sample the SPEC §5.D étage 3 gate requires). The committed JSONL is the frozen
artifact; this tool is how it is (re)produced.

Usage (servers already up)::

    python -m fluence_data.generate --teacher-url http://127.0.0.1:8080 \\
        --embed-url http://127.0.0.1:8090 --out corpus/v1.jsonl --per-cell 2
"""

from __future__ import annotations

import argparse
import json
import sys
import urllib.request
from collections.abc import Sequence
from pathlib import Path

from fluence_data.antipathos import pathos_findings
from fluence_data.formats import Dialogue, Speaker, Split, dump_jsonl
from fluence_data.teacher import (
    DEFAULT_DEDUP_THRESHOLD,
    DraftBatch,
    GenerationConfig,
    finalize_corpus,
    generate_drafts,
)

#: Fraction of the corpus surfaced for mandatory human anti-pathos review.
REVIEW_FRACTION = 0.10


def _post(url: str, payload: dict[str, object], *, timeout: float) -> str:
    """POST JSON and return the response body text."""
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"content-type": "application/json"},
    )
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return str(response.read().decode("utf-8"))


class LlamaTeacher:
    """A :class:`~fluence_data.teacher.TeacherClient` backed by ``llama-server``."""

    def __init__(
        self,
        url: str,
        *,
        temperature: float = 0.7,
        seed: int = 20260613,
        max_tokens: int = 256,
        timeout: float = 120.0,
    ) -> None:
        """Configure the chat-completion client (one local ``llama-server``)."""
        self._chat_url = url.rstrip("/") + "/v1/chat/completions"
        self._temperature = temperature
        self._seed = seed
        self._max_tokens = max_tokens
        self._timeout = timeout

    def complete(self, system: str, user: str) -> str:
        """Return the assistant reply to a ``(system, user)`` message pair."""
        payload: dict[str, object] = {
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            "temperature": self._temperature,
            "max_tokens": self._max_tokens,
            "seed": self._seed,
            "stream": False,
        }
        data = json.loads(_post(self._chat_url, payload, timeout=self._timeout))
        return str(data["choices"][0]["message"]["content"])


class LlamaEmbedder:
    """An embedding client backed by ``llama-server`` ``/v1/embeddings`` (bge-m3)."""

    def __init__(self, url: str, *, timeout: float = 120.0) -> None:
        """Configure the embedding client (one local ``llama-server``)."""
        self._embed_url = url.rstrip("/") + "/v1/embeddings"
        self._timeout = timeout

    def embed(self, texts: Sequence[str]) -> list[list[float]]:
        """Return one embedding vector per input text, in order."""
        payload: dict[str, object] = {"input": list(texts)}
        data = json.loads(_post(self._embed_url, payload, timeout=self._timeout))
        return [[float(value) for value in item["embedding"]] for item in data["data"]]


def render_dialogue(dialogue: Dialogue) -> str:
    """Render a dialogue as readable text for the human review file."""
    header = (
        f"[{dialogue.id}] {dialogue.situation.value} / "
        f"{dialogue.register.value} / {dialogue.split.value}"
    )
    lines = [
        f"  {'MOI' if turn.speaker is Speaker.USER else 'AUTRE'} : {turn.text}"
        for turn in dialogue.turns
    ]
    return "\n".join([header, *lines])


def review_sample(dialogues: Sequence[Dialogue], *, fraction: float = REVIEW_FRACTION) -> list[str]:
    """Pick the ids of the first ``fraction`` of each split for human review.

    Sampling per split (deterministic: every n-th by sorted id) guarantees the
    review spans train, dev and test rather than clustering in one partition.
    """
    by_split: dict[Split, list[Dialogue]] = {}
    for dialogue in dialogues:
        by_split.setdefault(dialogue.split, []).append(dialogue)
    sampled: list[str] = []
    for split_dialogues in by_split.values():
        ordered = sorted(split_dialogues, key=lambda dialogue: dialogue.id)
        count = max(1, round(len(ordered) * fraction))
        step = max(1, len(ordered) // count)
        sampled.extend(dialogue.id for dialogue in ordered[::step][:count])
    return sorted(sampled)


def write_review_file(dialogues: Sequence[Dialogue], path: Path) -> list[str]:
    """Write the human review file and return the residual pathos-flagged ids.

    The file lists every dialogue, marks the ≥ 10 % review sample, and calls out
    any residual anti-pathos finding (the obvious ones were dropped at
    generation; this catches the subtle framing a human must still confirm).
    """
    sample = set(review_sample(dialogues))
    flagged: list[str] = []
    blocks: list[str] = []
    for dialogue in sorted(dialogues, key=lambda dialogue: dialogue.id):
        findings = sorted({m for turn in dialogue.turns for m in pathos_findings(turn.text)})
        marks = []
        if dialogue.id in sample:
            marks.append("REVIEW")
        if findings:
            marks.append("PATHOS? " + ", ".join(findings))
            flagged.append(dialogue.id)
        suffix = f"   <<< {' | '.join(marks)}" if marks else ""
        blocks.append(render_dialogue(dialogue) + suffix)
    intro = (
        "# Revue anti-pathos (SPEC §5.D étage 3) — lis au moins les blocs marqués «REVIEW».\n"
        "# Un «PATHOS?» signale un marqueur résiduel : confirme ou corrige à la main.\n"
        f"# {len(sample)} dialogues échantillonnés sur {len(dialogues)} "
        f"({REVIEW_FRACTION:.0%} minimum).\n"
    )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(intro + "\n" + "\n\n".join(blocks) + "\n", encoding="utf-8")
    return flagged


def _report(batch: DraftBatch, dialogues: Sequence[Dialogue]) -> str:
    """A short generation report (attempts, rejections, kept, split counts)."""
    counts = {split: sum(1 for d in dialogues if d.split is split) for split in Split}
    split_line = ", ".join(f"{split.value}={counts[split]}" for split in Split)
    return (
        f"attempted={batch.attempted} "
        f"rejected(no_user={batch.rejected_no_user}, pathos={batch.rejected_pathos}) "
        f"accepted={len(batch.drafts)} -> after dedup+splits={len(dialogues)}\n"
        f"splits: {split_line}"
    )


def cmd_generate(args: argparse.Namespace) -> int:
    """Generate the corpus tranche, write the JSONL and the review file."""
    teacher = LlamaTeacher(args.teacher_url, temperature=args.temperature, seed=args.seed)
    embedder = LlamaEmbedder(args.embed_url)
    config = GenerationConfig(per_cell=args.per_cell, dedup_threshold=args.dedup_threshold)

    print(f"generating {len(config.matrix)} cells x {config.per_cell}...", file=sys.stderr)
    batch = generate_drafts(teacher, config)
    if not batch.drafts:
        print("no drafts accepted — is the teacher server up?", file=sys.stderr)
        return 1

    vectors = embedder.embed([draft.user_text for draft in batch.drafts])
    dialogues = finalize_corpus(
        batch.drafts,
        vectors,
        dedup_threshold=config.dedup_threshold,
        noise_seed=config.noise_seed,
    )

    dump_jsonl(dialogues, args.out)
    review_path = args.review_out or args.out.with_suffix(".review.txt")
    flagged = write_review_file(dialogues, review_path)

    print(_report(batch, dialogues))
    print(f"wrote {len(dialogues)} dialogues -> {args.out}")
    print(f"review file ({REVIEW_FRACTION:.0%} sample) -> {review_path}")
    if flagged:
        print(f"residual pathos markers in {len(flagged)} dialogues: {', '.join(flagged)}")
    return 0


def main(argv: list[str] | None = None) -> int:
    """Parse arguments and run the corpus generation."""
    parser = argparse.ArgumentParser(prog="fluence_data.generate", description=__doc__)
    parser.add_argument("--teacher-url", default="http://127.0.0.1:8080", help="teacher base URL")
    parser.add_argument("--embed-url", default="http://127.0.0.1:8090", help="embedding base URL")
    parser.add_argument(
        "--out", type=Path, default=Path("corpus/v1.jsonl"), help="output JSONL path"
    )
    parser.add_argument(
        "--review-out", type=Path, default=None, help="review file (default: <out>.review.txt)"
    )
    parser.add_argument("--per-cell", type=int, default=2, help="dialogues per matrix cell")
    parser.add_argument(
        "--dedup-threshold", type=float, default=DEFAULT_DEDUP_THRESHOLD, help="near-dup cosine"
    )
    parser.add_argument("--temperature", type=float, default=0.7, help="teacher temperature")
    parser.add_argument("--seed", type=int, default=20260613, help="teacher sampling seed")
    parser.set_defaults(func=cmd_generate)
    args = parser.parse_args(argv)
    result: int = args.func(args)
    return result


if __name__ == "__main__":
    sys.exit(main())
