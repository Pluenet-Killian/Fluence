# SPDX-License-Identifier: Apache-2.0
"""Input-variant generators (SPEC §5.D étage 1).

For each user turn, the corpus carries the *intended* text plus noisy
renderings the engine must reconstruct:

* **telegraphic** — function words dropped (« je voudrais de l'eau » → « voudrais
  eau »): the terse style of someone minimising keystrokes.
* **noised** — AZERTY spatial slips, omissions and doublings: eye-typing motor
  noise (uses :mod:`fluence_data.azerty`).
* **abbreviated** — French SMS/abbreviation rules: known forms plus a
  consonant-skeleton fallback for long words.

All three are deterministic (``noised`` through a seeded RNG), so a regenerated
corpus diffs cleanly against its committed form.
"""

from __future__ import annotations

import random
import unicodedata

from fluence_data.azerty import sample_keypress
from fluence_data.formats import InputVariant, VariantKind

#: French function words dropped by the telegraphic generator (closed classes:
#: articles, prepositions, pronouns, conjunctions, common auxiliaries).
_FUNCTION_WORDS: frozenset[str] = frozenset(
    {
        "le",
        "la",
        "les",
        "un",
        "une",
        "des",
        "de",
        "du",
        "au",
        "aux",
        "à",
        "je",
        "tu",
        "il",
        "elle",
        "on",
        "nous",
        "vous",
        "ils",
        "elles",
        "me",
        "te",
        "se",
        "lui",
        "leur",
        "y",
        "en",
        "ce",
        "cet",
        "cette",
        "ces",
        "ça",
        "cela",
        "mon",
        "ma",
        "mes",
        "ton",
        "ta",
        "tes",
        "son",
        "sa",
        "ses",
        "notre",
        "votre",
        "et",
        "ou",
        "mais",
        "donc",
        "or",
        "ni",
        "car",
        "que",
        "qui",
        "est",
        "es",
        "suis",
        "sommes",
        "êtes",
        "sont",
        "ai",
        "as",
        "a",
        "avons",
        "avez",
        "ont",
        "pour",
        "par",
        "avec",
        "sans",
        "dans",
        "sur",
        "sous",
        "chez",
        "vers",
    }
)

#: Known French abbreviations, applied as whole-token (lowercased) replacements.
_ABBREVIATIONS: dict[str, str] = {
    "bonjour": "bjr",
    "salut": "slt",
    "beaucoup": "bcp",
    "aujourd'hui": "ajd",
    "pourquoi": "pq",
    "parce": "pcq",
    "quelque": "qq",
    "quelqu'un": "qqn",
    "rendez-vous": "rdv",
    "longtemps": "lgtps",
    "s'il": "stp",
    "merci": "mci",
    "rien": "rien",
    "demain": "dem",
    "maintenant": "mnt",
    "tout": "tt",
    "toujours": "tjs",
}

_VOWELS = frozenset("aeiouyàâäéèêëîïôöùûü")

#: Elided clitics stripped before matching the content word (« l'eau » → « eau »).
_ELISIONS = ("qu'", "l'", "d'", "j'", "s'", "n'", "m'", "t'", "c'")


def _strip_elision(token: str) -> str:
    """Drop a leading elided clitic: « l'eau » → « eau », « j'ai » → « ai »."""
    lower = token.lower()
    for prefix in _ELISIONS:
        if lower.startswith(prefix):
            return token[len(prefix) :]
    return token


def _normalise(token: str) -> str:
    """Lowercase and strip surrounding punctuation for word matching."""
    return token.lower().strip('.,!?;:«»"()')


def telegraphic(text: str) -> str:
    """Drop function words, keeping content words in order (SPEC §5.D).

    Args:
        text: The intended sentence.

    Returns:
        The telegraphic rendering (may be empty if the input is all function
        words).
    """
    kept: list[str] = []
    for token in text.split():
        content = _strip_elision(token)
        if _normalise(content) in _FUNCTION_WORDS:
            continue
        stripped = content.strip('.,!?;:«»"()')
        if stripped:
            kept.append(stripped)
    return " ".join(kept)


def noised(text: str, rng: random.Random, *, error_rate: float = 0.08) -> str:
    """Apply AZERTY spatial slips, with rare omissions and doublings.

    Args:
        text: The intended sentence.
        rng: Seeded RNG — the sole source of nondeterminism.
        error_rate: Per-character spatial-slip probability (eye-typing noise).

    Returns:
        The noised rendering (the input unchanged when ``error_rate`` is 0).
    """
    if error_rate <= 0.0:
        return text
    out: list[str] = []
    for char in text:
        roll = rng.random()
        if roll < 0.03:  # omission: the key was missed entirely
            continue
        pressed = sample_keypress(char, error_rate, rng)
        out.append(pressed)
        if rng.random() < 0.02:  # doubling: the key registered twice
            out.append(pressed)
    return "".join(out)


def _abbreviate_token(token: str) -> str:
    """Abbreviate one token: known form, else consonant skeleton if long."""
    normal = token.lower()
    if normal in _ABBREVIATIONS:
        return _ABBREVIATIONS[normal]
    # Consonant-skeleton fallback for long words: keep the first letter, then
    # drop interior vowels (a common French shorthand). Short words stay whole.
    if len(token) <= 4:
        return token
    decomposed = unicodedata.normalize("NFD", token)
    head = token[0]
    body = "".join(
        ch for ch in decomposed[1:] if unicodedata.normalize("NFC", ch).lower() not in _VOWELS
    )
    skeleton = head + unicodedata.normalize("NFC", body)
    return skeleton if len(skeleton) >= 2 else token


def abbreviated(text: str) -> str:
    """Apply French abbreviation rules token by token (SPEC §5.D).

    Args:
        text: The intended sentence.

    Returns:
        The abbreviated rendering.
    """
    return " ".join(_abbreviate_token(token) for token in text.split())


def build_variants(text: str, rng: random.Random) -> list[InputVariant]:
    """Generate the input variants of a user turn, keeping only useful ones.

    A variant equal to the source (it added nothing) or empty (e.g. a
    telegraphic turn of only function words) is dropped. Shared by the
    hand-authored seed (:mod:`fluence_data.corpus_v0`) and the teacher-generated
    tranche (:mod:`fluence_data.teacher`) so both render variants identically.

    Args:
        text: The intended user-turn text.
        rng: Seeded RNG — the sole source of nondeterminism (``noised``).

    Returns:
        The useful input variants, in telegraphic / noised / abbreviated order.
    """
    candidates = (
        (VariantKind.TELEGRAPHIC, telegraphic(text)),
        (VariantKind.NOISED, noised(text, rng)),
        (VariantKind.ABBREVIATED, abbreviated(text)),
    )
    return [
        InputVariant(kind=kind, text=value) for kind, value in candidates if value and value != text
    ]
