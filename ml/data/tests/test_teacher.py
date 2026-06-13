# SPDX-License-Identifier: Apache-2.0
"""T1 — teacher-corpus generation logic: parsing, dedup, splits (#18)."""

from __future__ import annotations

import pytest

from fluence_data.formats import Register, Situation, Speaker, Split, Turn
from fluence_data.teacher import (
    DraftDialogue,
    GenerationConfig,
    _splits_for_count,
    cosine,
    dedup_by_embedding,
    finalize_corpus,
    generate_drafts,
    generation_prompt,
    parse_transcript,
)


class _ScriptedTeacher:
    """A teacher that returns canned replies in order (no network)."""

    def __init__(self, replies: list[str]) -> None:
        self._replies = replies
        self._index = 0

    def complete(self, system: str, user: str) -> str:
        reply = self._replies[self._index]
        self._index += 1
        return reply


def test_generation_prompt_names_situation_register_and_format() -> None:
    prompt = generation_prompt(Situation.RENDEZ_VOUS_MEDICAL, Register.FORMEL)
    assert "rendez-vous médical" in prompt
    assert "formel" in prompt
    assert "MOI" in prompt and "AUTRE" in prompt


def test_parse_transcript_extracts_speakers_and_strips_labels() -> None:
    turns = parse_transcript("MOI : j'ai faim\nAUTRE : on mange dans dix minutes")
    assert [turn.speaker for turn in turns] == [Speaker.USER, Speaker.PARTNER]
    assert turns[0].text == "j'ai faim"
    assert turns[1].text == "on mange dans dix minutes"


def test_parse_transcript_skips_non_label_lines_and_bullets() -> None:
    raw = "Voici le dialogue :\n- MOI : salut\n> AUTRE: ça va ?\nFin du dialogue."
    turns = parse_transcript(raw)
    assert [turn.text for turn in turns] == ["salut", "ça va ?"]


def test_parse_transcript_strips_surrounding_quotes() -> None:
    assert parse_transcript('MOI : "bonjour"')[0].text == "bonjour"


def test_parse_transcript_truncates_to_max_turns() -> None:
    raw = "\n".join(f"MOI : ligne {i}" for i in range(10))
    assert len(parse_transcript(raw)) == 6


def test_generate_drafts_counts_rejections() -> None:
    config = GenerationConfig(
        per_cell=1,
        matrix=(
            (Situation.SOINS, Register.INTIME),
            (Situation.REPAS, Register.FAMILIER),
            (Situation.FAMILLE, Register.NEUTRE),
        ),
    )
    teacher = _ScriptedTeacher(
        [
            "MOI : j'ai un peu faim",  # accepted
            "AUTRE : bonjour",  # no user turn → rejected
            "MOI : quel courage tu as",  # pathos marker → rejected
        ]
    )
    batch = generate_drafts(teacher, config)
    assert batch.attempted == 3
    assert len(batch.drafts) == 1
    assert batch.rejected_no_user == 1
    assert batch.rejected_pathos == 1
    assert batch.drafts[0].situation is Situation.SOINS
    assert batch.drafts[0].register is Register.INTIME


def test_cosine_extremes() -> None:
    assert cosine([1.0, 0.0], [1.0, 0.0]) == pytest.approx(1.0)
    assert cosine([1.0, 0.0], [0.0, 1.0]) == pytest.approx(0.0)
    assert cosine([0.0, 0.0], [1.0, 1.0]) == 0.0  # zero vector is safe


def test_dedup_by_embedding_keeps_first_drops_near_duplicate() -> None:
    vectors = [[1.0, 0.0], [1.0, 0.0], [0.0, 1.0]]
    assert dedup_by_embedding(vectors, threshold=0.9) == [0, 2]


def test_splits_for_count_always_holds_out_a_test_dialogue() -> None:
    assert _splits_for_count(0) == []
    assert _splits_for_count(1) == [Split.TRAIN]
    assert _splits_for_count(2) == [Split.TRAIN, Split.TEST]
    assert _splits_for_count(8) == [Split.TRAIN] * 6 + [Split.DEV, Split.TEST]
    # Every bucket of two or more keeps at least one held-out test dialogue.
    for count in range(2, 30):
        assert Split.TEST in _splits_for_count(count)


def _user(text: str) -> Turn:
    return Turn(speaker=Speaker.USER, text=text)


def _partner(text: str) -> Turn:
    return Turn(speaker=Speaker.PARTNER, text=text)


def _drafts_and_vectors() -> tuple[list[DraftDialogue], list[list[float]]]:
    drafts = [
        DraftDialogue(Situation.SOINS, Register.INTIME, (_user("je voudrais de l'eau fraîche"),)),
        DraftDialogue(Situation.SOINS, Register.NEUTRE, (_partner("ça va"), _user("pas trop mal"))),
        DraftDialogue(Situation.SOINS, Register.FORMEL, (_user("je voudrais qu'on ajuste tout"),)),
        DraftDialogue(Situation.SOINS, Register.FAMILIER, (_user("ramène le chien dimanche"),)),
        DraftDialogue(Situation.REPAS, Register.FORMEL, (_user("le poisson du jour sans sauce"),)),
    ]
    vectors = [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 1.0],
    ]
    return drafts, vectors


def test_finalize_corpus_assigns_ids_splits_and_variants() -> None:
    drafts, vectors = _drafts_and_vectors()
    dialogues = finalize_corpus(drafts, vectors)

    ids = [dialogue.id for dialogue in dialogues]
    assert ids == ["v1-repas-01", "v1-soins-01", "v1-soins-02", "v1-soins-03", "v1-soins-04"]

    by_id = {dialogue.id: dialogue for dialogue in dialogues}
    # A four-dialogue situation holds out one dev and one test.
    assert [by_id[f"v1-soins-0{n}"].split for n in (1, 2, 3, 4)] == [
        Split.TRAIN,
        Split.TRAIN,
        Split.DEV,
        Split.TEST,
    ]
    assert by_id["v1-repas-01"].split is Split.TRAIN

    # User turns carry variants; partner turns never do.
    soins01 = by_id["v1-soins-01"]
    assert soins01.turns[0].speaker is Speaker.USER
    assert soins01.turns[0].variants  # non-empty
    repas = by_id["v1-repas-01"]
    assert all(not t.variants for t in repas.turns if t.speaker is Speaker.PARTNER)


def test_finalize_corpus_is_deterministic() -> None:
    drafts, vectors = _drafts_and_vectors()
    first = [d.model_dump_json() for d in finalize_corpus(drafts, vectors)]
    second = [d.model_dump_json() for d in finalize_corpus(drafts, vectors)]
    assert first == second


def test_finalize_corpus_drops_near_duplicates() -> None:
    drafts, vectors = _drafts_and_vectors()
    drafts.append(DraftDialogue(Situation.REPAS, Register.NEUTRE, (_user("autre chose"),)))
    vectors.append([0.0, 1.0, 1.0])  # identical to the last REPAS vector → dropped
    dialogues = finalize_corpus(drafts, vectors)
    assert len(dialogues) == len(drafts) - 1


def test_finalize_corpus_rejects_length_mismatch() -> None:
    drafts, vectors = _drafts_and_vectors()
    with pytest.raises(ValueError, match="must align"):
        finalize_corpus(drafts, vectors[:-1])
