# SPDX-License-Identifier: Apache-2.0
"""Teacher-LLM corpus generation (SPEC §5.D étage 1; issue #18).

The seed corpus v0 is hand-authored and tiny; scaling to the controlled
generation matrix (12 situations × 4 registers, strict anti-pathos consigne)
needs a teacher LLM. This module turns that into reproducible code: a
client-agnostic orchestrator — the LLM is injected behind :class:`TeacherClient`,
the embedder behind a plain vector list — plus pure, tested helpers for prompt
building, transcript parsing, embedding dedup and frozen split assignment.

The teacher's text is non-deterministic, so the *artifact* is the truth: a
generation run writes a versioned JSONL (e.g. ``corpus/v1.jsonl``) that is
committed and frozen. This module is the tool that produces it; the tests pin
its parsing / dedup / splitting against fakes — **no network in the pure layer**
(the real HTTP clients live in :mod:`fluence_data.generate`).

The output is 100 % synthetic (D-8.2): it carries no personal data, so it is not
P0 — it may be logged, published (CC BY-SA) and stored in the clear.
"""

from __future__ import annotations

import math
import random
import re
from collections.abc import Sequence
from dataclasses import dataclass, field
from typing import Protocol

from fluence_data.antipathos import pathos_findings
from fluence_data.formats import (
    Dialogue,
    Register,
    Situation,
    Speaker,
    Split,
    Turn,
)
from fluence_data.variants import build_variants

#: Default seed for the variant RNG of a generated corpus (stable artifact).
DEFAULT_NOISE_SEED = 20260613
#: Default near-duplicate cosine threshold: drop a dialogue this close to a kept
#: one. High on purpose — only genuine paraphrase collisions are removed.
DEFAULT_DEDUP_THRESHOLD = 0.92
#: Keep at most this many turns per generated dialogue (tidy, bounds runaway).
MAX_TURNS = 6

#: Human-readable French labels for the generation prompt (the enum values are
#: slugs; these read naturally in the consigne).
SITUATION_LABEL: dict[Situation, str] = {
    Situation.SOINS: "les soins du quotidien",
    Situation.REPAS: "un repas",
    Situation.FAMILLE: "la famille",
    Situation.RENDEZ_VOUS_MEDICAL: "un rendez-vous médical",
    Situation.URGENCE: "une urgence",
    Situation.LOISIRS: "les loisirs",
    Situation.DEMARCHES_ADMIN: "une démarche administrative",
    Situation.VISITE_AMI: "la visite d'un ami",
    Situation.TELEPHONE: "un appel téléphonique",
    Situation.COUPLE: "la vie de couple",
    Situation.AIDE_DOMICILE: "l'aide à domicile",
    Situation.SORTIE: "une sortie",
}
REGISTER_LABEL: dict[Register, str] = {
    Register.INTIME: "intime",
    Register.FAMILIER: "familier",
    Register.NEUTRE: "neutre",
    Register.FORMEL: "formel",
}

#: The full generation matrix: every situation × every register (SPEC §5.D).
GENERATION_MATRIX: tuple[tuple[Situation, Register], ...] = tuple(
    (situation, register) for situation in Situation for register in Register
)

#: The anti-pathos consigne (SPEC §5.D étage 3) given to the teacher as the
#: system message: ordinary life, never misérabilisme / inspiration porn.
ANTIPATHOS_SYSTEM = (
    "Tu écris des dialogues en français pour entraîner un clavier de communication "
    "assistée destiné à des personnes ayant un handicap moteur lourd (par exemple la "
    "SLA). La personne, notée « MOI », s'exprime normalement : envies, humour, "
    "agacement, demandes concrètes, vie ordinaire. "
    "INTERDIT ABSOLU : misérabilisme, pitié, « courage », « malgré le handicap », "
    "héroïsme, leçon de vie, inspiration. La personne n'est pas un symbole ni un "
    "exemple : c'est quelqu'un d'ordinaire. Aucun méta-commentaire sur le handicap. "
    "Des phrases naturelles, parfois brèves ou imparfaites, comme une vraie conversation."
)

#: Speaker labels the teacher must use, and how they map back to the schema.
_USER_LABELS = ("moi", "je", "user", "patient")
_PARTNER_LABELS = ("autre", "partner", "partenaire", "interlocuteur")
_LABEL_RE = re.compile(r"^[\s\-*>•]*([a-zA-Zàâäéèêëîïôöùûüç]+)\s*[:.)\-]\s*(.+)$")


class TeacherClient(Protocol):
    """A teacher LLM behind a single chat-completion call (injected; faked in tests)."""

    def complete(self, system: str, user: str) -> str:
        """Return the assistant's reply to a ``(system, user)`` message pair."""
        ...


@dataclass(frozen=True)
class DraftDialogue:
    """A parsed dialogue before ids, splits and input variants are assigned."""

    situation: Situation
    register: Register
    turns: tuple[Turn, ...]

    @property
    def user_text(self) -> str:
        """The user turns joined — the text deduplication embeds on."""
        return " ".join(turn.text for turn in self.turns if turn.speaker is Speaker.USER)


@dataclass(frozen=True)
class DraftBatch:
    """The outcome of one generation pass, with rejection counts for reporting."""

    drafts: list[DraftDialogue] = field(default_factory=list)
    attempted: int = 0
    rejected_no_user: int = 0
    rejected_pathos: int = 0


@dataclass(frozen=True)
class GenerationConfig:
    """Knobs for a generation run (defaults target the ~100-dialogue tranche)."""

    per_cell: int = 2
    dedup_threshold: float = DEFAULT_DEDUP_THRESHOLD
    noise_seed: int = DEFAULT_NOISE_SEED
    matrix: tuple[tuple[Situation, Register], ...] = GENERATION_MATRIX


def generation_prompt(situation: Situation, register: Register) -> str:
    """Build the per-cell user prompt for one (situation, register) cell."""
    return (
        f"Situation : {SITUATION_LABEL[situation]}. "
        f"Registre : {REGISTER_LABEL[register]}.\n"
        "Écris un court échange naturel (2 à 5 répliques) entre MOI (la personne) et "
        "AUTRE (l'interlocuteur), avec au moins une réplique de MOI.\n"
        "Format STRICT : une réplique par ligne, préfixée par « MOI : » ou « AUTRE : ». "
        "Aucune autre ligne, pas de numérotation, pas de commentaire."
    )


def _speaker_of(label: str) -> Speaker | None:
    """Map a line label to a speaker, or ``None`` if it is not a known role."""
    folded = label.strip().lower()
    if folded in _USER_LABELS:
        return Speaker.USER
    if folded in _PARTNER_LABELS:
        return Speaker.PARTNER
    return None


def parse_transcript(raw: str) -> list[Turn]:
    """Parse a ``MOI:`` / ``AUTRE:`` transcript into turns (best-effort, robust).

    Lines without a recognised speaker label are skipped, so stray preamble or
    trailing commentary from the teacher is ignored rather than corrupting the
    dialogue. Surrounding quotes are stripped; the result is truncated to
    :data:`MAX_TURNS`.

    Args:
        raw: The teacher's raw reply.

    Returns:
        The parsed turns, in order (possibly empty).
    """
    turns: list[Turn] = []
    for line in raw.splitlines():
        match = _LABEL_RE.match(line.strip())
        if match is None:
            continue
        speaker = _speaker_of(match.group(1))
        if speaker is None:
            continue
        text = match.group(2).strip().strip("\"'«»").strip()
        if text:
            turns.append(Turn(speaker=speaker, text=text))
    return turns[:MAX_TURNS]


def _draft_or_reason(
    situation: Situation, register: Register, turns: list[Turn]
) -> DraftDialogue | str:
    """Build a draft, or return a rejection reason (``"no_user"`` / ``"pathos"``)."""
    if not any(turn.speaker is Speaker.USER for turn in turns):
        return "no_user"
    if any(pathos_findings(turn.text) for turn in turns):
        return "pathos"
    return DraftDialogue(situation=situation, register=register, turns=tuple(turns))


def generate_drafts(client: TeacherClient, config: GenerationConfig) -> DraftBatch:
    """Call the teacher across the matrix and collect the accepted drafts.

    Drafts with no user turn, or that trip an anti-pathos marker, are dropped
    and counted (the obvious misérabilisme is filtered automatically; a human
    still reviews a sample of the survivors, SPEC §5.D étage 3).

    Args:
        client: The teacher LLM.
        config: The generation knobs (matrix, per-cell count…).

    Returns:
        The accepted drafts plus rejection counts for the run report.
    """
    drafts: list[DraftDialogue] = []
    attempted = no_user = pathos = 0
    for situation, register in config.matrix:
        prompt = generation_prompt(situation, register)
        for _ in range(config.per_cell):
            attempted += 1
            turns = parse_transcript(client.complete(ANTIPATHOS_SYSTEM, prompt))
            outcome = _draft_or_reason(situation, register, turns)
            if isinstance(outcome, DraftDialogue):
                drafts.append(outcome)
            elif outcome == "no_user":
                no_user += 1
            else:
                pathos += 1
    return DraftBatch(
        drafts=drafts,
        attempted=attempted,
        rejected_no_user=no_user,
        rejected_pathos=pathos,
    )


def cosine(a: Sequence[float], b: Sequence[float]) -> float:
    """Cosine similarity of two vectors (0.0 when either is the zero vector)."""
    dot = sum(x * y for x, y in zip(a, b, strict=True))
    norm_a = math.sqrt(sum(x * x for x in a))
    norm_b = math.sqrt(sum(y * y for y in b))
    if norm_a == 0.0 or norm_b == 0.0:
        return 0.0
    return dot / (norm_a * norm_b)


def dedup_by_embedding(vectors: Sequence[Sequence[float]], *, threshold: float) -> list[int]:
    """Greedy near-duplicate removal; return kept indices in input order.

    The first occurrence is always kept; a later item is dropped when its cosine
    to any kept item reaches ``threshold``.

    Args:
        vectors: One embedding per candidate, in candidate order.
        threshold: Cosine at or above which a later item is a duplicate.

    Returns:
        The indices to keep, ascending.
    """
    kept: list[int] = []
    for index, vector in enumerate(vectors):
        if all(cosine(vector, vectors[other]) < threshold for other in kept):
            kept.append(index)
    return kept


def _splits_for_count(count: int) -> list[Split]:
    """Frozen train/dev/test labels for a situation bucket of ``count`` dialogues.

    Roughly 75 / 12.5 / 12.5, with at least one held-out test dialogue as soon
    as the bucket has two — the test split must never be empty for a situation
    that has any data (it is what the value gate measures, ADR-0008).
    """
    if count <= 0:
        return []
    if count == 1:
        return [Split.TRAIN]
    if count == 2:
        return [Split.TRAIN, Split.TEST]
    n_test = max(1, count // 6)
    n_dev = max(1, count // 6)
    n_train = count - n_test - n_dev
    return [Split.TRAIN] * n_train + [Split.DEV] * n_dev + [Split.TEST] * n_test


def finalize_corpus(
    drafts: Sequence[DraftDialogue],
    vectors: Sequence[Sequence[float]],
    *,
    dedup_threshold: float = DEFAULT_DEDUP_THRESHOLD,
    noise_seed: int = DEFAULT_NOISE_SEED,
) -> list[Dialogue]:
    """Deduplicate, assign frozen splits and ids, attach variants — pure.

    Given the drafts and a parallel list of their user-text embeddings, this is
    fully deterministic, so a regenerated corpus diffs cleanly. Splits are
    assigned per situation so every situation present is represented in train and
    (when it has ≥ 2 dialogues) in the held-out test split.

    Args:
        drafts: The accepted drafts, in generation order.
        vectors: ``drafts[i]``'s user-text embedding at index ``i``.
        dedup_threshold: Cosine at or above which a draft is a near-duplicate.
        noise_seed: Seed for the variant RNG (stable artifact).

    Returns:
        The finalized dialogues, sorted by id.

    Raises:
        ValueError: If ``drafts`` and ``vectors`` differ in length.
    """
    if len(drafts) != len(vectors):
        msg = f"drafts ({len(drafts)}) and vectors ({len(vectors)}) must align"
        raise ValueError(msg)

    kept = [drafts[i] for i in dedup_by_embedding(vectors, threshold=dedup_threshold)]

    by_situation: dict[Situation, list[DraftDialogue]] = {}
    for draft in kept:
        by_situation.setdefault(draft.situation, []).append(draft)

    rng = random.Random(noise_seed)
    dialogues: list[Dialogue] = []
    for situation in Situation:  # stable enum order → deterministic ids
        bucket = by_situation.get(situation, [])
        splits = _splits_for_count(len(bucket))
        for number, (draft, split) in enumerate(zip(bucket, splits, strict=True), start=1):
            turns = [
                Turn(
                    speaker=turn.speaker,
                    text=turn.text,
                    variants=build_variants(turn.text, rng) if turn.speaker is Speaker.USER else [],
                )
                for turn in draft.turns
            ]
            dialogues.append(
                Dialogue(
                    id=f"v1-{situation.value}-{number:02d}",
                    situation=situation,
                    register=draft.register,
                    split=split,
                    turns=turns,
                    notes="teacher-generated v1 (Gemma 4 E4B)",
                )
            )
    return sorted(dialogues, key=lambda dialogue: dialogue.id)
