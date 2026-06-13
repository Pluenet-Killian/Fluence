# SPDX-License-Identifier: Apache-2.0
"""T1 — the anti-pathos grille flags framing clichés, not ordinary difficulty."""

import pytest

from fluence_data import is_pathos_free, pathos_findings


def test_misérabilisme_clichés_are_flagged() -> None:
    findings = pathos_findings("Quel courage, malgré son handicap, une vraie source d'inspiration.")
    assert "quel courage" in findings
    assert "malgre son handicap" in findings
    assert "source d'inspiration" in findings
    assert not is_pathos_free("il est si courageux")


def test_flagging_is_accent_insensitive() -> None:
    # Accents must not let a cliché slip through.
    assert pathos_findings("une belle leçon de vie") == ["lecon de vie"]


@pytest.mark.parametrize(
    "ordinary",
    [
        "tu peux me passer le sel",
        "j'ai mal au dos ce matin",  # an ordinary complaint, not pathos framing
        "je suis fatigué, on rentre",
        "raconte-moi ton week-end",
    ],
)
def test_ordinary_life_is_not_flagged(ordinary: str) -> None:
    assert is_pathos_free(ordinary)
