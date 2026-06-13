# SPDX-License-Identifier: Apache-2.0
"""T1 — corpus v0: coverage, anti-pathos, determinism, and the committed golden."""

from pathlib import Path

from fluence_data import (
    Register,
    Situation,
    Speaker,
    Split,
    VariantKind,
    build_corpus_v0,
    is_pathos_free,
    load_jsonl,
)

CORPUS_PATH = Path(__file__).parents[1] / "corpus" / "v0.jsonl"


def test_corpus_builds_and_is_non_empty() -> None:
    corpus = build_corpus_v0()
    assert len(corpus) >= 12  # at least one dialogue per situation


def test_all_twelve_situations_are_covered() -> None:
    situations = {dialogue.situation for dialogue in build_corpus_v0()}
    assert situations == set(Situation)


def test_all_four_registers_are_used() -> None:
    registers = {dialogue.register for dialogue in build_corpus_v0()}
    assert registers == set(Register)


def test_every_split_is_represented() -> None:
    splits = {dialogue.split for dialogue in build_corpus_v0()}
    assert splits == set(Split)


def test_corpus_is_pathos_free() -> None:
    # The whole seed must pass the anti-pathos grille (SPEC §5.D consigne).
    for dialogue in build_corpus_v0():
        for turn in dialogue.turns:
            assert is_pathos_free(turn.text), f"pathos in {dialogue.id}: {turn.text!r}"


def test_every_user_turn_carries_variants_of_each_kind() -> None:
    kinds_seen: set[VariantKind] = set()
    for dialogue in build_corpus_v0():
        for turn in dialogue.user_turns:
            assert turn.variants, f"user turn without variants in {dialogue.id}"
            kinds_seen.update(variant.kind for variant in turn.variants)
    assert kinds_seen == set(VariantKind)  # all three kinds appear in the corpus


def test_build_is_deterministic() -> None:
    assert build_corpus_v0() == build_corpus_v0()


def test_committed_jsonl_matches_the_builder() -> None:
    # The golden artifact must equal a fresh build — regenerate it when the
    # builder or seed changes (like the contract goldens).
    assert load_jsonl(CORPUS_PATH) == build_corpus_v0()


def test_partner_turns_never_carry_variants() -> None:
    for dialogue in build_corpus_v0():
        for turn in dialogue.turns:
            if turn.speaker is Speaker.PARTNER:
                assert turn.variants == []
