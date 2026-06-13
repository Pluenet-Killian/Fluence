# Changelog

Toutes les évolutions notables du projet, par phase d'exécution (PLAN §2).
Format inspiré de [Keep a Changelog](https://keepachangelog.com/fr/) ; le
projet est en pré-alpha, sans release publiée (les jalons A1/B1/1.0 sont
définis en SPEC D-12.2).

## Phase 1 — Le contrat (2026-06-13)

### Ajouté

- `fluence-protocol` complet : 100 % des messages et endpoints des SPEC
  §4.A (FluenceInput v1), §5.A (API du hub), §5.B (mémoire), §2.A
  (appairage, scopes, WebSocket) — invariants dans les types (`Normalized`
  rejette les hors-bornes à la désérialisation), enveloppe d'erreur
  RFC 9457 avec catalogue de codes stables.
- Registre de routes déclaratif (`routes()`) : la surface API comme données
  testées (unicité, préfixe, scopes, stabilité `stable`/`experimental`).
- Chaîne de contrats `cargo xtask check-contracts` : 28 goldens JSON Schema
  + OpenAPI 3.1 + types TS générés (`openapi-typescript`), dérive = erreur
  de CI (job `contracts (T3)`, lint spectral à zéro), prouvée par mutation.
- `@fluence/sdk` v0 : client typé (fetch + SSE + WebSocket), zéro logique
  métier, erreurs problem+json typées, parser SSE robuste à la
  fragmentation, 17 tests dont tests de types.
- Doc API publiée sur GitHub Pages (Redoc + rustdoc) ; gate de couverture
  `fluence-protocol` ≥ 85 % en CI.
- ADR-0004 (décisions de contrat v1).

## Phase 0 — L'usine (2026-06-13)

### Ajouté

- Monorepo §2.B : workspaces Cargo (7 crates + xtask), pnpm (3 packages),
  uv (3 packages ml) — chaque unité testée, qualité bloquante (clippy
  pedantic, `forbid(unsafe_code)`, TS strict, mypy strict).
- Licences par couche (D-10.1) : briques Apache-2.0, applications
  AGPL-3.0-only ; en-têtes SPDX vérifiés par `cargo xtask check-licenses`
  (le dépôt se vérifie lui-même) ; `cargo-deny` bloquant.
- 4 workflows CI (ci, integration, nightly, release dry-run) en matrice
  Windows + Linux ; protections de branche (squash-only, checks requis) ;
  hooks lefthook < 5 s ; conventional commits vérifiés (hook + CI).
- Gouvernance : CONTRIBUTING, SECURITY (divulgation coordonnée, D-9.3),
  CODE_OF_CONDUCT, templates PR/issues (dont `debt`), template ADR.
- ADR-0001 (hub/workers), ADR-0002 (monorepo & contrats), ADR-0003
  (outillage Phase 0).
