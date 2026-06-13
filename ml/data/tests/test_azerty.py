# SPDX-License-Identifier: Apache-2.0
"""T1 — AZERTY confusion model: distribution properties + seeded determinism."""

import random

import pytest

from fluence_data import confusion_distribution, neighbours, sample_keypress


def test_no_error_rate_means_no_confusion() -> None:
    assert confusion_distribution("f", 0.0) == {"f": 1.0}


def test_off_grid_characters_are_never_confused() -> None:
    # Space, punctuation, accented letters are not on the v0 grid.
    assert confusion_distribution(" ", 0.3) == {" ": 1.0}
    assert confusion_distribution("é", 0.3) == {"é": 1.0}


def test_distribution_sums_to_one() -> None:
    total = sum(confusion_distribution("f", 0.3).values())
    assert total == pytest.approx(1.0)


def test_intended_key_keeps_the_majority_mass() -> None:
    dist = confusion_distribution("f", 0.3)
    assert dist["f"] == pytest.approx(0.7)
    assert all(dist["f"] > p for key, p in dist.items() if key != "f")


def test_a_closer_neighbour_is_likelier_than_a_farther_one() -> None:
    # On AZERTY 'd' is horizontally adjacent to 'f' (distance 1.0); 't' is a
    # staggered diagonal farther away. The closer slip must be likelier.
    dist = confusion_distribution("f", 0.3)
    assert dist["d"] > dist["t"]


def test_distant_keys_get_no_mass() -> None:
    dist = confusion_distribution("f", 0.3)
    assert "p" not in dist  # opposite side of the keyboard
    assert "a" not in dist


def test_neighbours_excludes_self_and_distant_keys() -> None:
    nbrs = neighbours("f")
    assert "f" not in nbrs
    assert "d" in nbrs and "g" in nbrs  # horizontal pair
    assert "p" not in nbrs


@pytest.mark.parametrize("bad_rate", [-0.01, 1.01, 2.0])
def test_out_of_range_error_rate_is_rejected(bad_rate: float) -> None:
    with pytest.raises(ValueError, match="error_rate"):
        confusion_distribution("f", bad_rate)


def test_sampling_is_deterministic_for_a_given_seed() -> None:
    word = "bonjour"
    first = [sample_keypress(c, 0.3, random.Random(42)) for c in word]
    second = [sample_keypress(c, 0.3, random.Random(42)) for c in word]
    assert first == second


def test_sampling_without_noise_returns_the_intended_key() -> None:
    rng = random.Random(0)
    assert [sample_keypress(c, 0.0, rng) for c in "salut"] == list("salut")
