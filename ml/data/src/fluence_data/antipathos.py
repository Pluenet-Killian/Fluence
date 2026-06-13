# SPDX-License-Identifier: Apache-2.0
"""Anti-pathos review grille (SPEC §5.D étage 3).

LLMs caricature disability when ungoverned (« courage », « malgré son
handicap », inspiration porn). The corpus consigne is the opposite: ordinary
life — humour, irritation, desire — *not* misérabilisme (SPEC §5.D). This module
is the **published grille** turned into an automatic flagger: it surfaces the
framing clichés for human review (SPEC asks for ≥ 10 % manual review against
the grille), deliberately matching the *framing* of disability, not legitimate
expressions of difficulty (« j'ai mal », « je suis fatigué » are ordinary life).

The flagger is a v0 heuristic aid, not a hard gate; a hit means « a human should
look », not « reject ».
"""

from __future__ import annotations

import unicodedata

#: Framing clichés of misérabilisme / inspiration porn (SPEC §5.D). Accent- and
#: case-insensitive substring markers. Kept to *framing*, not symptom words, so
#: an ordinary mention of pain or fatigue is not flagged.
PATHOS_MARKERS: tuple[str, ...] = (
    "malgre son handicap",
    "malgre sa maladie",
    "malgre la maladie",
    "prisonnier de son corps",
    "prisonniere de son corps",
    "enferme dans son corps",
    "courageux",
    "courageuse",
    "quel courage",
    "force d'ame",
    "lecon de vie",
    "lecon d'humilite",
    "source d'inspiration",
    "inspirant",
    "inspirante",
    "battant",
    "se bat contre",
    "combat contre la maladie",
    "victime de son",
    "fardeau",
    "pauvre petit",
    "pauvre homme",
    "pauvre femme",
    "miracle de la vie",
    "heros du quotidien",
)


def _fold(text: str) -> str:
    """Lowercase and strip accents for accent-insensitive matching."""
    decomposed = unicodedata.normalize("NFD", text.lower())
    return "".join(ch for ch in decomposed if not unicodedata.combining(ch))


def pathos_findings(text: str) -> list[str]:
    """Return the pathos markers present in ``text`` (review aid, SPEC §5.D).

    Args:
        text: The text to screen.

    Returns:
        The matched markers, in grille order (empty when none — the desired
        state for the corpus).
    """
    folded = _fold(text)
    return [marker for marker in PATHOS_MARKERS if marker in folded]


def is_pathos_free(text: str) -> bool:
    """Whether ``text`` trips no pathos marker (SPEC §5.D anti-pathos consigne)."""
    return not pathos_findings(text)
