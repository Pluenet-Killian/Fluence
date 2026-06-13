# SPDX-License-Identifier: Apache-2.0
"""Versioned corpus format — dialogues, turns, input variants (SPEC §5.D, §8.A).

This is the public schema of FluenceBench-FR (D-8.3): a corpus is a stream of
synthetic French dialogues, each a sequence of turns between the AAC ``user``
(the person we accelerate) and a ``partner``. Every *user* turn carries its
ground-truth text plus optional noisy input renderings (telegraphic, AZERTY
spatial noise, abbreviated) that later modes (rephrase/expand) are scored on.

Invariants live in the types (mirroring ``fluence-protocol``'s philosophy):
a partner turn cannot carry input variants, a turn cannot be empty, variant
kinds are unique per turn — so every layer above can trust a loaded corpus.

The corpus is **100 % synthetic** (D-8.2): it contains no personal data, so it
is *not* P0 — it may be logged, published (CC BY-SA), and stored in the clear.
"""

from __future__ import annotations

from enum import StrEnum
from pathlib import Path

from pydantic import BaseModel, ConfigDict, Field, field_validator, model_validator

#: Corpus schema version. A backward-incompatible change increments it; the
#: public FluenceBench-FR goldens pin this value.
SCHEMA_VERSION = 1


class Situation(StrEnum):
    """The 12 everyday situations of the generation matrix (SPEC §5.D)."""

    SOINS = "soins"
    REPAS = "repas"
    FAMILLE = "famille"
    RENDEZ_VOUS_MEDICAL = "rendez_vous_medical"
    URGENCE = "urgence"
    LOISIRS = "loisirs"
    DEMARCHES_ADMIN = "demarches_admin"
    VISITE_AMI = "visite_ami"
    TELEPHONE = "telephone"
    COUPLE = "couple"
    AIDE_DOMICILE = "aide_domicile"
    SORTIE = "sortie"


class Register(StrEnum):
    """The 4 registers, from most to least intimate (SPEC §5.D)."""

    INTIME = "intime"
    FAMILIER = "familier"
    NEUTRE = "neutre"
    FORMEL = "formel"


class Speaker(StrEnum):
    """Who speaks a turn."""

    #: The AAC user — the person whose typing we accelerate and measure.
    USER = "user"
    #: The interlocutor (caregiver, relative, clinician…).
    PARTNER = "partner"


class VariantKind(StrEnum):
    """A noisy rendering of a user turn's intended text (SPEC §5.D étage 1)."""

    #: Function words dropped: « eau frache stp ».
    TELEGRAPHIC = "telegraphic"
    #: AZERTY spatial-confusion noise: key neighbours, omissions, doublings.
    NOISED = "noised"
    #: French abbreviation rules: initials, consonant skeleton, SMS forms.
    ABBREVIATED = "abbreviated"


class Split(StrEnum):
    """Frozen dataset partition. The test split never tunes prompts (SPEC §5.D)."""

    TRAIN = "train"
    DEV = "dev"
    TEST = "test"


class InputVariant(BaseModel):
    """One noisy rendering of a user turn's intended text."""

    model_config = ConfigDict(frozen=True, extra="forbid")

    kind: VariantKind
    text: str = Field(min_length=1)

    @field_validator("text")
    @classmethod
    def _text_is_not_blank(cls, value: str) -> str:
        """Reject whitespace-only variant text."""
        if not value.strip():
            msg = "variant text must not be blank"
            raise ValueError(msg)
        return value


class Turn(BaseModel):
    """One conversational turn.

    A speaker and their text, plus — for user turns only — the noisy input
    renderings (telegraphic/noised/abbreviated) the engine reconstructs.
    """

    model_config = ConfigDict(extra="forbid")

    speaker: Speaker
    #: Ground-truth text actually said this turn (synthetic; not P0).
    text: str = Field(min_length=1)
    #: Input renderings — present only on user turns.
    variants: list[InputVariant] = Field(default_factory=list)

    @field_validator("text")
    @classmethod
    def _text_is_not_blank(cls, value: str) -> str:
        """Reject whitespace-only turn text."""
        if not value.strip():
            msg = "turn text must not be blank"
            raise ValueError(msg)
        return value

    @model_validator(mode="after")
    def _variants_are_consistent(self) -> Turn:
        """Variants belong to user turns only, with at most one of each kind."""
        if self.variants and self.speaker is not Speaker.USER:
            msg = "only user turns may carry input variants"
            raise ValueError(msg)
        kinds = [variant.kind for variant in self.variants]
        if len(kinds) != len(set(kinds)):
            msg = "input variant kinds must be unique within a turn"
            raise ValueError(msg)
        return self


class Dialogue(BaseModel):
    """A complete multi-turn dialogue in one situation and register."""

    model_config = ConfigDict(extra="forbid")

    schema_version: int = SCHEMA_VERSION
    id: str = Field(min_length=1)
    situation: Situation
    register: Register
    split: Split
    turns: list[Turn] = Field(min_length=1)
    #: Free-form provenance/datasheet note (e.g. "hand-authored seed v0").
    notes: str | None = None

    @field_validator("schema_version")
    @classmethod
    def _schema_version_is_known(cls, value: int) -> int:
        """Reject a dialogue from a future, unknown schema."""
        if value != SCHEMA_VERSION:
            msg = f"unsupported corpus schema_version {value} (this build speaks {SCHEMA_VERSION})"
            raise ValueError(msg)
        return value

    @property
    def user_turns(self) -> list[Turn]:
        """The user's turns, in order — what the harness types and measures."""
        return [turn for turn in self.turns if turn.speaker is Speaker.USER]


def load_jsonl(path: Path) -> list[Dialogue]:
    """Load a JSONL corpus file (one :class:`Dialogue` per line).

    Args:
        path: Path to the ``.jsonl`` file.

    Returns:
        The dialogues, validated. Blank lines are skipped.

    Raises:
        pydantic.ValidationError: If any line violates the schema.
    """
    dialogues: list[Dialogue] = []
    with path.open(encoding="utf-8") as handle:
        for line in handle:
            stripped = line.strip()
            if stripped:
                dialogues.append(Dialogue.model_validate_json(stripped))
    return dialogues


def dump_jsonl(dialogues: list[Dialogue], path: Path) -> None:
    """Write dialogues to a JSONL file, one compact JSON object per line.

    The output is deterministic (stable key order from the model definition),
    so a regenerated corpus diffs cleanly against its committed form.

    Args:
        dialogues: The dialogues to serialise.
        path: Destination ``.jsonl`` path (parents created if absent).
    """
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="\n") as handle:
        for dialogue in dialogues:
            handle.write(dialogue.model_dump_json())
            handle.write("\n")


def split_of(dialogues: list[Dialogue], split: Split) -> list[Dialogue]:
    """Return the dialogues assigned to ``split`` (frozen partition, SPEC §5.D)."""
    return [dialogue for dialogue in dialogues if dialogue.split is split]
