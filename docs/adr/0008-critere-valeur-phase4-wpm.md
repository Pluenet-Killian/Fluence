# ADR-0008 — Critère de valeur Phase 4 (#31) : WPM primaire + KS% hors-domaine

- **Statut** : accepté
- **Date** : 2026-06-13
- **Décisions SPEC liées** : étoile polaire WPM (SPEC §1.2), D-8.1 (harnais §8.A), D-5.3 (API hub) ; PLAN Phase 4 T6 ; issues #31 (valeur), #18 (corpus).

## Contexte

L'ingénierie Phase 4 est livrée et branchée (backend `llama-server`, supervision, `/suggest` SSE, fallback n-gram). Le critère de valeur restant (#31, PLAN Phase 4 T6) gate sur : *« rephrase bat le n-gram d'au moins +10 points de KS% sur le corpus v0 »*.

La mesure réelle, une fois le **gabarit de chat appliqué** dans le backend (`generate` → `/v1/chat/completions`, sinon le modèle instruct *bavarde* — voir le commit du fix), donne sur le seed v0 (Gemma 4 E4B, acceptation sémantique par embeddings bge-m3, seuil cosinus 0,80) :

| mode | KS% | WPM | acceptation |
|---|---|---|---|
| lettre-à-lettre | 0,00 | 15,00 | 0,00 |
| n-gram | 35,49 | 15,87 | 0,46 |
| rephrase | 19,58 | **18,35** | **0,93** |

Deux artefacts rendent le gate KS%-only **structurellement inatteignable et non informatif sur le seed** :

1. **Le KS% favorise la complétion de mots.** Le n-gram complète des mots en cours de frappe (gros gain KS%). La reformulation part d'un fragment télégraphique dont la longueur **plafonne** le KS% : sur le corpus v0, les fragments ne sont courts qu'à ~26 % de la cible ⇒ KS% rephrase ≤ ~26 %, sous le n-gram.
2. **Le n-gram du seed est sur-appris (in-domain).** Il est entraîné *et* évalué sur les mêmes 15 dialogues ⇒ 35,49 % gonflé, non représentatif (le PLAN session 3 le notait déjà : « 35 % in-domain, pas une revendication socle »).

Or l'**étoile polaire** du produit (SPEC §1.2) est le **débit conversationnel effectif (WPM ×3)**, pas le KS%. Sur le WPM, rephrase **gagne déjà** (18,35 vs 15,87) : produire une phrase entière d'un fragment minimal est plus rapide que compléter mot à mot.

## Options considérées

1. **Garder le gate KS%-only sur le seed** — inatteignable (plafond < n-gram gonflé) ; le « réussir » exigerait d'ajuster un seuil pour faire passer un build, interdit par PLAN §0.8.
2. **Amender vers WPM primaire + KS% hors-domaine secondaire**, mesuré sur une tranche corpus représentative (#18) avec n-gram entraîné hors-domaine et fragments réalistement courts.
3. **KS% hors-domaine seul** sur la tranche — ignore que le WPM est le vrai objectif et reste sujet au plafond de longueur du fragment.

## Décision

Nous choisissons l'**option 2**. Le critère de valeur #31 devient :

> **rephrase bat le n-gram (a) sur le WPM simulé — *primaire*, l'étoile polaire ×3 (SPEC §1.2) — et (b) sur le KS% mesuré *hors-domaine*** (n-gram entraîné sur le split *train*, évalué sur *test* ; fragments télégraphiques réalistement courts), sur la **tranche corpus #18 (~100 dialogues, splits gelés)**.

parce que le KS%-only in-domain sur le seed est un proxy biaisé (n-gram sur-appris + borne de longueur du fragment) qui ne mesure pas le gain réel. Ce n'est **pas** un ajustement de seuil pour faire passer un build (PLAN §0.8) mais une **correction méthodologique explicite** (PLAN §0.5 : un conflit proxy↔réalité se résout par amendement documenté, jamais en silence).

## Conséquences

- **Plus juste** : la mesure reflète le gain conversationnel réel (WPM) et compare équitablement (n-gram hors-domaine, jamais évalué sur ses données d'entraînement).
- **Plus contraint** : exige (i) la **tranche #18** (génération teacher + splits gelés + fragments terses), (ii) une mesure **split-aware** dans `fluence_eval.measure` (n-gram entraîné sur train, tous les modes évalués sur test). Surveillé par le runner local ; gate CI nightly à la mise en place du runner self-hosted (Phase 7).
- **Le socle A1 inchangé** : D-12.2 (KS% ≥ 25 % *hors-domaine* sur le corpus v1) reste la cible A1 ; cet amendement aligne seulement le proxy #31 sur l'étoile polaire et l'équité hors-domaine.
- **Dette restante** : corpus v1 ≥ 500 dialogues par teacher (#18 complet) et gate CI nightly (runner self-hosted, Phase 7).
- **SPEC** : aucun amendement SPEC nécessaire — le WPM est déjà l'étoile polaire (§1.2) ; on y aligne le proxy. PLAN §2 (Phase 4 T6 + « Done quand ») et §5 amendés en conséquence.
