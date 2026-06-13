# SPDX-License-Identifier: Apache-2.0
"""Hand-authored seed corpus v0 (SPEC §5.D, PLAN 3.4; ADR-0006).

A small, genuine French seed covering the 12 situations across the 4 registers,
written to the anti-pathos consigne (ordinary life: humour, desire, irritation,
plain requests — no misérabilisme). It is enough to validate the harness and
bracket a real source; **scaling to the full ~100-dialogue corpus by a teacher
LLM (SPEC §5.D étage 1) is deferred** (no local teacher — ADR-0006), tracked as
debt.

:func:`build_corpus_v0` attaches the input variants (telegraphic / noised /
abbreviated) to every user turn deterministically, so the committed
``corpus/v0.jsonl`` is reproducible. Splits are frozen here, in the data — the
test split never tunes anything.
"""

from __future__ import annotations

import random
from dataclasses import dataclass

from fluence_data.formats import (
    Dialogue,
    Register,
    Situation,
    Speaker,
    Split,
    Turn,
)
from fluence_data.variants import build_variants

#: Seed for the noised-variant RNG. Fixed so the corpus artifact is stable.
_NOISE_SEED = 20260613


@dataclass(frozen=True)
class _Seed:
    """An authored dialogue before its input variants are generated."""

    id: str
    situation: Situation
    register: Register
    split: Split
    turns: tuple[tuple[Speaker, str], ...]


_U = Speaker.USER
_P = Speaker.PARTNER

#: The authored seed. One dialogue per situation, registers spread across all
#: four, every line ordinary life (anti-pathos). Splits frozen.
_SEEDS: tuple[_Seed, ...] = (
    _Seed(
        "v0-soins-01",
        Situation.SOINS,
        Register.INTIME,
        Split.TRAIN,
        ((_P, "Tu as bien dormi ?"), (_U, "pas trop mal mais j'ai encore le dos raide")),
    ),
    _Seed(
        "v0-repas-01",
        Situation.REPAS,
        Register.FAMILIER,
        Split.TRAIN,
        (
            (_P, "Qu'est-ce qui te ferait plaisir ce midi ?"),
            (_U, "des pâtes avec beaucoup de fromage"),
            (_U, "et un verre de vin si tu veux bien"),
        ),
    ),
    _Seed(
        "v0-repas-02",
        Situation.REPAS,
        Register.FORMEL,
        Split.DEV,
        ((_P, "Vous avez choisi ?"), (_U, "je prendrai le poisson du jour sans sauce")),
    ),
    _Seed(
        "v0-famille-01",
        Situation.FAMILLE,
        Register.FAMILIER,
        Split.TRAIN,
        ((_P, "Les enfants passent dimanche."), (_U, "super dis-leur d'amener le chien")),
    ),
    _Seed(
        "v0-famille-02",
        Situation.FAMILLE,
        Register.INTIME,
        Split.TEST,
        ((_U, "tu me manques quand tu pars travailler le matin"),),
    ),
    _Seed(
        "v0-medical-01",
        Situation.RENDEZ_VOUS_MEDICAL,
        Register.FORMEL,
        Split.TRAIN,
        (
            (_P, "Comment vous sentez-vous depuis la dernière fois ?"),
            (_U, "la fatigue augmente surtout l'après-midi"),
            (_U, "je voudrais qu'on ajuste le traitement"),
        ),
    ),
    _Seed(
        "v0-urgence-01",
        Situation.URGENCE,
        Register.NEUTRE,
        Split.TRAIN,
        ((_U, "appelle vite je n'arrive plus à respirer correctement"),),
    ),
    _Seed(
        "v0-loisirs-01",
        Situation.LOISIRS,
        Register.FAMILIER,
        Split.TRAIN,
        ((_P, "On regarde quoi ce soir ?"), (_U, "le match puis un film si je tiens")),
    ),
    _Seed(
        "v0-loisirs-02",
        Situation.LOISIRS,
        Register.INTIME,
        Split.DEV,
        ((_U, "viens on écoute notre chanson préférée"),),
    ),
    _Seed(
        "v0-admin-01",
        Situation.DEMARCHES_ADMIN,
        Register.FORMEL,
        Split.TRAIN,
        (
            (_U, "je vous appelle pour renouveler mon dossier"),
            (_P, "Il manque une pièce au formulaire."),
            (_U, "je vous envoie l'attestation demain matin"),
        ),
    ),
    _Seed(
        "v0-visite-01",
        Situation.VISITE_AMI,
        Register.FAMILIER,
        Split.TRAIN,
        (
            (_P, "Ça fait plaisir de te voir !"),
            (_U, "raconte ton voyage en Italie c'était comment"),
        ),
    ),
    _Seed(
        "v0-telephone-01",
        Situation.TELEPHONE,
        Register.NEUTRE,
        Split.TEST,
        ((_U, "allô c'est moi tu peux parler deux minutes"),),
    ),
    _Seed(
        "v0-couple-01",
        Situation.COUPLE,
        Register.INTIME,
        Split.TRAIN,
        ((_P, "Tu m'en veux encore ?"), (_U, "non mais préviens-moi la prochaine fois")),
    ),
    _Seed(
        "v0-aide-01",
        Situation.AIDE_DOMICILE,
        Register.NEUTRE,
        Split.DEV,
        ((_P, "Je commence par la cuisine ?"), (_U, "plutôt la salle de bain d'abord merci")),
    ),
    _Seed(
        "v0-sortie-01",
        Situation.SORTIE,
        Register.FAMILIER,
        Split.TEST,
        ((_P, "On sort prendre l'air ?"), (_U, "oui au parc j'ai envie de soleil")),
    ),
)


def build_corpus_v0() -> list[Dialogue]:
    """Build the seed corpus with variants attached (deterministic).

    Returns:
        The dialogues of corpus v0, every user turn carrying its input
        variants. Reproducible: the noised variant uses a fixed seed.
    """
    rng = random.Random(_NOISE_SEED)
    dialogues: list[Dialogue] = []
    for seed in _SEEDS:
        turns = [
            Turn(
                speaker=speaker,
                text=text,
                variants=build_variants(text, rng) if speaker is Speaker.USER else [],
            )
            for speaker, text in seed.turns
        ]
        dialogues.append(
            Dialogue(
                id=seed.id,
                situation=seed.situation,
                register=seed.register,
                split=seed.split,
                turns=turns,
                notes="hand-authored seed v0",
            )
        )
    return dialogues
