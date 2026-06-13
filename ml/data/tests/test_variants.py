# SPDX-License-Identifier: Apache-2.0
"""T1 — input-variant generators: deterministic FR rules + AZERTY noise."""

import random

from fluence_data import abbreviated, noised, telegraphic


def test_telegraphic_drops_function_words() -> None:
    assert telegraphic("je voudrais de l'eau fraîche") == "voudrais eau fraîche"


def test_telegraphic_keeps_content_only_phrases_intact() -> None:
    # Pure content words survive unchanged.
    assert telegraphic("eau fraîche maintenant") == "eau fraîche maintenant"


def test_telegraphic_of_only_function_words_is_empty() -> None:
    assert telegraphic("je le lui en") == ""


def test_abbreviated_applies_known_forms_and_consonant_skeleton() -> None:
    # bonjour/beaucoup are known SMS forms; "fromage" falls back to a
    # consonant skeleton; a short word is left whole.
    assert abbreviated("bonjour beaucoup fromage oui") == "bjr bcp frmg oui"


def test_noised_is_identity_without_noise() -> None:
    assert noised("bonjour ça va", random.Random(0), error_rate=0.0) == "bonjour ça va"


def test_noised_is_deterministic_for_a_given_seed() -> None:
    text = "je voudrais des nouvelles"
    assert noised(text, random.Random(7)) == noised(text, random.Random(7))


def test_noised_actually_perturbs_with_noise() -> None:
    # With a high error rate the output should differ from the input (it is the
    # eye-typing-noise variant after all).
    text = "rendez-vous demain matin"
    assert noised(text, random.Random(1), error_rate=0.5) != text
