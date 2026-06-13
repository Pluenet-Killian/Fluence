# Changelog

Toutes les évolutions notables du projet, par phase d'exécution (PLAN §2).
Format inspiré de [Keep a Changelog](https://keepachangelog.com/fr/) ; le
projet est en pré-alpha, sans release publiée (les jalons A1/B1/1.0 sont
définis en SPEC D-12.2).

## Phase 2 — Hub & supervision : « le clavier parle toujours » (2026-06-13)

### Ajouté

- `fluence-ipc` : couche IPC hub↔workers — frames JSON préfixées longueur
  (cap 16 MiB) sur UDS (Linux) / named pipes (Windows) derrière une API
  unique ; protocole v0 (Hello/Ping/Echo/Shutdown). ADR-0005.
- `fluence-store` : persistance chiffrée SQLCipher (AES-256) ; clé en
  keystore OS (ou fichier en test) ; migrations versionnées ;
  acteur mono-thread à file de commandes (ordre d'écriture garanti) ;
  WAL + `synchronous=FULL` (durabilité ≤ 1 s, D-2.6) ; tokens stockés
  hashés (SHA-256) ; journal d'accès sans donnée P0.
- `fluence-hub` : bootstrap (< 3 s, config TOML+env, port 7411 + repli
  dynamique), tracing à **redaction P0** (types `SecretString` + denylist
  de champs), arrêt propre ; appairage (fenêtre 2 min, code à usage unique,
  verrouillage anti-brute-force → 429), tokens à scopes
  (display/control/care/system), middleware d'auth (401 uniforme), CORS
  allowlist stricte ; superviseur (backoff exponentiel + jitter, états,
  événements `system.degraded`) ; WebSocket `/ws` multiplexé par topics
  filtrés par scope ; autosave du draft ; `/system/health` +
  `/system/capabilities` ; worker `worker-echo` pour les kill-tests.
- Routes ajoutées au contrat via la chaîne anti-dérive : `POST /pair/window`
  (system), `GET /sessions/{id}/draft` (reprise de session §2.A).

### Vérifié (kill-tests, le cœur de la phase)

- worker tué → `system.degraded` < 500 ms (provisional) + relance à backoff
  + compteur de restarts exposé ; hub tué (-9) en pleine frappe → draft
  restauré, perte ≤ 1 s ; 50 cycles kill/restart → RSS stable (±10 %) ;
  démarrage → prêt < 3 s ; aucun contenu P0 dans les logs (bout en bout).

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
