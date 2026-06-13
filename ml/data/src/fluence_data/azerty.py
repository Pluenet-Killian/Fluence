# SPDX-License-Identifier: Apache-2.0
"""AZERTY spatial confusion model (SPEC §5.D, §8.A).

Eye-typing errors are *spatial*: a missed target lands on a neighbouring key,
not a random one (SPEC §5.D). This module places the French AZERTY letters on
a staggered grid and turns that geometry into a per-character confusion
distribution — used both to generate the « noised » input variant (corpus)
and to model motor noise in the simulated user (harness).

The distribution is pure and deterministic; only :func:`sample_keypress` draws
from it, through an explicitly seeded RNG, so a whole run reproduces to the bit
given its seed.
"""

from __future__ import annotations

import random

# Letter rows of a French AZERTY keyboard, with each row's horizontal offset
# (keys are staggered like a real keyboard, so neighbours are diagonal).
_ROWS: tuple[tuple[str, float], ...] = (
    ("azertyuiop", 0.0),
    ("qsdfghjklm", 0.25),
    ("wxcvbn", 0.75),
)

#: Two keys are neighbours when their centres are within this distance. Tuned
#: so a key's horizontal pair (distance 1.0) and its staggered diagonals
#: (≈ 1.03–1.12) count, while the next-but-one key (≥ 1.6) does not.
_NEIGHBOUR_RADIUS = 1.3


def _build_coords() -> dict[str, tuple[float, float]]:
    """Map each AZERTY letter to its ``(x, y)`` grid centre."""
    coords: dict[str, tuple[float, float]] = {}
    for row_index, (letters, offset) in enumerate(_ROWS):
        for col_index, letter in enumerate(letters):
            coords[letter] = (col_index + offset, float(row_index))
    return coords


_COORDS: dict[str, tuple[float, float]] = _build_coords()


def neighbours(char: str) -> dict[str, float]:
    """Return the spatial neighbours of ``char`` and their distances.

    Args:
        char: A single lowercase letter on the AZERTY grid.

    Returns:
        ``{neighbour: distance}`` for every key within
        :data:`_NEIGHBOUR_RADIUS` (excluding ``char`` itself). Empty for a
        character not on the grid.
    """
    here = _COORDS.get(char)
    if here is None:
        return {}
    result: dict[str, float] = {}
    for other, (ox, oy) in _COORDS.items():
        if other == char:
            continue
        distance = ((ox - here[0]) ** 2 + (oy - here[1]) ** 2) ** 0.5
        if distance <= _NEIGHBOUR_RADIUS:
            result[other] = distance
    return result


def confusion_distribution(char: str, error_rate: float) -> dict[str, float]:
    """Probability distribution over the key actually pressed for ``char``.

    The intended key keeps ``1 − error_rate``; the ``error_rate`` mass is split
    among the spatial neighbours, weighted by inverse distance (a closer key is
    a likelier slip than a farther one). A character off the grid — space,
    punctuation, an accented letter not modelled in v0 — is always pressed
    correctly.

    Args:
        char: The intended character.
        error_rate: Slip probability in ``[0, 1]``.

    Returns:
        ``{pressed_char: probability}`` summing to 1.

    Raises:
        ValueError: If ``error_rate`` is outside ``[0, 1]``.
    """
    if not 0.0 <= error_rate <= 1.0:
        msg = f"error_rate must be in [0, 1], got {error_rate}"
        raise ValueError(msg)
    nbrs = neighbours(char)
    if error_rate == 0.0 or not nbrs:
        return {char: 1.0}
    inverse_weights = {key: 1.0 / distance for key, distance in nbrs.items()}
    total_weight = sum(inverse_weights.values())
    distribution = {char: 1.0 - error_rate}
    for key, weight in inverse_weights.items():
        distribution[key] = error_rate * weight / total_weight
    return distribution


def sample_keypress(char: str, error_rate: float, rng: random.Random) -> str:
    """Draw the key actually pressed for an intended ``char`` (seeded RNG).

    Args:
        char: The intended character.
        error_rate: Slip probability in ``[0, 1]``.
        rng: A seeded random source — the sole source of nondeterminism.

    Returns:
        The character pressed (``char`` itself, or a spatial neighbour).
    """
    distribution = confusion_distribution(char, error_rate)
    if len(distribution) == 1:
        return char
    draw = rng.random()
    cumulative = 0.0
    # Sorted for a stable cumulative order: same seed ⇒ same draw.
    for key, probability in sorted(distribution.items()):
        cumulative += probability
        if draw <= cumulative:
            return key
    return char
