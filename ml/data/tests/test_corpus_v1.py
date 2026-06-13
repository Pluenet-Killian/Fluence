# SPDX-License-Identifier: Apache-2.0
"""T1 — corpus v1 (teacher tranche, #18): validate the committed artifact.

Unlike v0, v1 is teacher-LLM-generated and not reproducible from a builder, so
these tests pin the *committed* JSONL's invariants: it parses, covers the
matrix, freezes splits with a held-out test per situation (so the value gate is
out-of-domain, ADR-0008), and stays anti-pathos clean.
"""

from pathlib import Path

from fluence_data import (
    Dialogue,
    Situation,
    Speaker,
    Split,
    VariantKind,
    is_pathos_free,
    load_jsonl,
)

CORPUS_PATH = Path(__file__).parents[1] / "corpus" / "v1.jsonl"


def _corpus() -> list[Dialogue]:
    return load_jsonl(CORPUS_PATH)


def test_committed_jsonl_parses_and_is_a_real_tranche() -> None:
    # ~100-dialogue tranche target (ADR-0008); the committed run holds 136.
    assert len(_corpus()) >= 100


def test_all_twelve_situations_are_covered() -> None:
    assert {dialogue.situation for dialogue in _corpus()} == set(Situation)


def test_every_split_is_represented() -> None:
    assert {dialogue.split for dialogue in _corpus()} == set(Split)


def test_every_situation_has_a_held_out_test_dialogue() -> None:
    # The out-of-domain measurement (ADR-0008) needs every situation present in
    # the test split, not only in train.
    test_situations = {d.situation for d in _corpus() if d.split is Split.TEST}
    assert test_situations == set(Situation)


def test_corpus_is_pathos_free() -> None:
    # The committed tranche must pass the anti-pathos grille (SPEC §5.D); the
    # human review of the sample is recorded in the datasheet.
    for dialogue in _corpus():
        for turn in dialogue.turns:
            assert is_pathos_free(turn.text), f"pathos in {dialogue.id}: {turn.text!r}"


def test_user_turns_carry_variants_and_all_kinds_appear() -> None:
    kinds_seen: set[VariantKind] = set()
    for dialogue in _corpus():
        for turn in dialogue.user_turns:
            assert turn.variants, f"user turn without variants in {dialogue.id}: {turn.text!r}"
            kinds_seen.update(variant.kind for variant in turn.variants)
    assert kinds_seen == set(VariantKind)


def test_partner_turns_never_carry_variants() -> None:
    for dialogue in _corpus():
        for turn in dialogue.turns:
            if turn.speaker is Speaker.PARTNER:
                assert turn.variants == []
