# Fluence

[![ci](https://github.com/Pluenet-Killian/Fluence/actions/workflows/ci.yml/badge.svg)](https://github.com/Pluenet-Killian/Fluence/actions/workflows/ci.yml)
[![integration](https://github.com/Pluenet-Killian/Fluence/actions/workflows/integration.yml/badge.svg)](https://github.com/Pluenet-Killian/Fluence/actions/workflows/integration.yml)
[![Briques : Apache-2.0](https://img.shields.io/badge/briques-Apache--2.0-blue.svg)](LICENSE.md)
[![Application : AGPL-3.0](https://img.shields.io/badge/application-AGPL--3.0-blue.svg)](LICENSE.md)

> **Plateforme de communication open source, locale et intégrée, pour le handicap moteur
> lourd (SLA en tête).**

Ramener une personne de 8–10 mots/minute à **30+ mots/minute effectifs en conversation
réelle** : entrée multimodale (regard, tête, contacteur — webcam comprise), accélération de
la communication par LLM local, voix personnelle clonée. **100 % hors-ligne, les données ne
quittent jamais le foyer.**

⚠️ **Pré-alpha.** Le projet démarre (Phase 0 : infrastructure). Rien d'utilisable encore —
le premier jalon utilisateur est l'alpha A1 (voir la feuille de route).

## La thèse

Personne n'occupe l'intersection des trois fronts : (a) intégration verticale entrée + IA +
voix, (b) IA **locale** de qualité (seuil franchi par les modèles ~4B), (c) open source
local-first. Analogie de positionnement : Home Assistant — le produit intégré local-first
qui devient à la fois la référence et l'écosystème. Le différenciateur défendable : *le
système apprend la personne (style, voix, proches, routines) et tout reste chez elle.*

## Documents

| Document | Rôle |
|---|---|
| [`docs/SPEC.md`](docs/SPEC.md) | Source de vérité produit & technique — décisions `D-x.y` actées |
| [`docs/PLAN.md`](docs/PLAN.md) | Guide d'exécution — règles, pyramide de tests T1–T6, phases 0–7, état |
| [`docs/adr/`](docs/adr/) | Décisions d'architecture (ADR) |
| [`CONTRIBUTING.md`](CONTRIBUTING.md) | Règles de contribution, mise en place locale |
| [`SECURITY.md`](SECURITY.md) | Signalement de vulnérabilités (divulgation coordonnée) |

## Structure

```
crates/     briques Rust : hub, protocol (★ source de vérité des schémas),
            inference, input, accel, voice, store          [Apache-2.0]
packages/   briques TypeScript : sdk, ui, integrations      [Apache-2.0]
ml/         pipelines Python : data, training, eval         [Apache-2.0]
apps/       application : desktop (Tauri), web-client, cli  [AGPL-3.0-only]
models/     manifestes signés du registre de modèles (jamais de poids en git)
docs/       SPEC, PLAN, ADR
```

## Principes non négociables

- **« Le clavier parle toujours »** : composer et vocaliser ne dépendent jamais de la santé
  des composants IA (workers isolés, repli n-gram, voix OS de secours).
- **Local-first chiffré** : les conversations, la mémoire et la voix (données P0) ne
  quittent jamais le foyer ; le cloud est un opt-in granulaire, jamais un défaut.
- **L'agentivité d'abord** : le système parle *comme la personne*, jamais à sa place ;
  tout est éditable et rejetable d'un geste.
- **Mesuré, pas promis** : chaque gain revendiqué passe par le harnais d'évaluation en CI
  (frappes économisées, WPM simulé), avec baselines et ablations.
