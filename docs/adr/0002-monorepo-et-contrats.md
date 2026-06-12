# ADR-0002 — Monorepo et contrats générés depuis une source unique

- **Statut** : accepté
- **Date** : 2026-06-13
- **Décisions SPEC liées** : D-2.5, D-5.3 (spec §2.B)

> Recopie des décisions SPEC dans le dépôt (PLAN tâche 0.6). En cas de
> divergence, la SPEC fait foi.

## Contexte

Trois écosystèmes (Rust, TypeScript, Python) doivent partager les mêmes
types : messages du protocole d'entrée (§4.A), API du hub (§5.A), formats
d'évaluation. En phase jeune, les refactorings transverses sont fréquents et
la dérive de contrat entre dépôts séparés serait un poison lent. Le harnais
d'évaluation doit tourner dans la même CI que le code qu'il mesure.

## Décision

- **Monorepo GitHub** : `crates/` (Cargo workspace), `apps/`, `packages/`
  (pnpm workspace), `ml/` (uv workspace), `models/` (manifestes), `docs/`.
  Les briques sont *publiées depuis* le monorepo (crates.io / npm) si des
  consommateurs externes apparaissent.
- **`fluence-protocol` est la source de vérité des schémas** : types Rust +
  `schemars` → JSON Schema (goldens commités) → OpenAPI 3.1 → types TS du
  SDK. `cargo xtask check-contracts` échoue en CI si les artefacts générés
  divergent (Phase 1).
- Erreurs API : `application/problem+json` (RFC 9457), catalogue de codes
  stables.
- CI en matrice Windows + Linux ; harnais d'éval en sous-ensemble rapide par
  PR, complet en nightly ; benchs de latence sur runners self-hosted
  (machines de référence D-11.1, Phase 7).
- Conventional commits ; trunk-based ; PR squash sur `main` protégée ;
  toute décision d'architecture nouvelle = un ADR ici.

## Conséquences

- La dérive de contrat devient une erreur de build, pas un bug de prod.
- Un seul clone, une seule CI, un seul historique — au prix d'un outillage
  multi-langages à la racine (assumé, c'est la Phase 0).
- Les niveaux de stabilité du contrat (`stable` vs `experimental`,
  PLAN tâche 1.3bis) permettent de figer tôt ce qu'on implémente tôt sans
  s'interdire d'apprendre sur le reste.
