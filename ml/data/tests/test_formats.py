# SPDX-License-Identifier: Apache-2.0
"""T1 — the corpus format keeps its invariants and round-trips through JSONL."""

from pathlib import Path

import pytest
from pydantic import ValidationError

from fluence_data import (
    SCHEMA_VERSION,
    Dialogue,
    InputVariant,
    Register,
    Situation,
    Speaker,
    Split,
    Turn,
    VariantKind,
    dump_jsonl,
    load_jsonl,
    split_of,
)


def _dialogue(**overrides: object) -> Dialogue:
    """A minimal valid dialogue, with optional field overrides."""
    base: dict[str, object] = {
        "id": "d-001",
        "situation": Situation.REPAS,
        "register": Register.FAMILIER,
        "split": Split.DEV,
        "turns": [
            Turn(speaker=Speaker.PARTNER, text="Tu veux quoi pour le dîner ?"),
            Turn(
                speaker=Speaker.USER,
                text="je voudrais des pâtes",
                variants=[
                    InputVariant(kind=VariantKind.TELEGRAPHIC, text="veux pâtes"),
                    InputVariant(kind=VariantKind.ABBREVIATED, text="je vdrai des pates"),
                ],
            ),
        ],
    }
    base.update(overrides)
    return Dialogue(**base)


def test_a_well_formed_dialogue_validates() -> None:
    dialogue = _dialogue()
    assert dialogue.schema_version == SCHEMA_VERSION
    assert [turn.speaker for turn in dialogue.turns] == [Speaker.PARTNER, Speaker.USER]


def test_user_turns_exposes_only_the_user_side() -> None:
    dialogue = _dialogue()
    user_turns = dialogue.user_turns
    assert len(user_turns) == 1
    assert user_turns[0].text == "je voudrais des pâtes"


def test_a_partner_turn_may_not_carry_input_variants() -> None:
    # Variants model the *user's* noisy input; a partner turn carrying them is
    # a corpus bug we reject at construction.
    with pytest.raises(ValidationError, match="only user turns"):
        Turn(
            speaker=Speaker.PARTNER,
            text="bonjour",
            variants=[InputVariant(kind=VariantKind.NOISED, text="bonjuor")],
        )


def test_duplicate_variant_kinds_are_rejected() -> None:
    with pytest.raises(ValidationError, match="unique"):
        Turn(
            speaker=Speaker.USER,
            text="oui",
            variants=[
                InputVariant(kind=VariantKind.NOISED, text="oiu"),
                InputVariant(kind=VariantKind.NOISED, text="ui"),
            ],
        )


@pytest.mark.parametrize("blank", ["", "   ", "\n\t"])
def test_blank_text_is_rejected(blank: str) -> None:
    with pytest.raises(ValidationError):
        Turn(speaker=Speaker.USER, text=blank)


def test_an_unknown_schema_version_is_rejected() -> None:
    # Forward incompatibility must fail loudly, never be silently misread.
    with pytest.raises(ValidationError, match="schema_version"):
        _dialogue(schema_version=SCHEMA_VERSION + 1)


def test_an_empty_dialogue_is_rejected() -> None:
    with pytest.raises(ValidationError):
        _dialogue(turns=[])


def test_jsonl_round_trips(tmp_path: Path) -> None:
    # Dump → load must reproduce the dialogues exactly (the corpus is a
    # committed artifact; a lossy round-trip would dirty diffs).
    dialogues = [
        _dialogue(id="d-001", split=Split.TRAIN),
        _dialogue(id="d-002", split=Split.TEST),
    ]
    path = tmp_path / "corpus.jsonl"
    dump_jsonl(dialogues, path)
    assert load_jsonl(path) == dialogues


def test_jsonl_dump_is_one_object_per_line_and_skips_blanks(tmp_path: Path) -> None:
    path = tmp_path / "corpus.jsonl"
    dump_jsonl([_dialogue(id="d-001"), _dialogue(id="d-002")], path)
    lines = path.read_text(encoding="utf-8").splitlines()
    assert len(lines) == 2

    # A stray blank line must not become a phantom dialogue.
    path.write_text(path.read_text(encoding="utf-8") + "\n\n", encoding="utf-8")
    assert len(load_jsonl(path)) == 2


def test_split_of_filters_the_frozen_partition() -> None:
    dialogues = [
        _dialogue(id="d-001", split=Split.TRAIN),
        _dialogue(id="d-002", split=Split.DEV),
        _dialogue(id="d-003", split=Split.TRAIN),
    ]
    train = split_of(dialogues, Split.TRAIN)
    assert [d.id for d in train] == ["d-001", "d-003"]
    assert split_of(dialogues, Split.TEST) == []
