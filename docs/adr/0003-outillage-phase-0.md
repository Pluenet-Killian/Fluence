# ADR-0003 — Décisions d'outillage de la Phase 0

- **Statut** : accepté
- **Date** : 2026-06-13
- **Décisions SPEC liées** : D-10.1 (licences) ; sinon décisions d'exécution
  (PLAN §0.5 : toute décision prise en cours de route est consignée)

## Contexte

La Phase 0 (« l'usine ») impose des choix d'outillage que ni la SPEC ni le
PLAN ne tranchent complètement. Chaque choix ci-dessous privilégie : zéro
magie, le moins de dépendances possible, un comportement identique sur
Windows et Linux.

## Décisions

1. **Toolchain Rust épinglée = MSRV = 1.89** (`rust-toolchain.toml`,
   edition 2024, resolver 3). Projet jeune : pas de promesse de support de
   versions antérieures ; toute montée de version est un commit explicite.
2. **Warnings bloquants via `RUSTFLAGS=-D warnings` en CI**, pas
   `#![deny(warnings)]` dans le code : l'intention du PLAN §0.6 (zéro warning
   sur main) sans casser les builds des utilisateurs futurs quand un nouveau
   rustc ajoute des lints. Clippy `pedantic` en `warn` au niveau workspace →
   bloquant en CI, vivable en local.
3. **`unsafe_code = "forbid"` hérité du workspace** ; une future crate FFI
   (`-sys`) définira sa propre table `[lints]` au lieu d'hériter — la règle
   du PLAN (« unsafe interdit hors crates FFI isolées ») devient structurelle.
4. **Vérification des licences par `cargo xtask check-licenses`** (maison,
   std pur) plutôt que REUSE : zéro dépendance externe, et la règle vérifie
   aussi l'architecture (du code source hors de `crates/`, `packages/`,
   `ml/`, `xtask/`, `apps/` est une violation).
5. **`ml/` et `xtask/` sous Apache-2.0** : D-10.1 ne cite pas ces racines ;
   le harnais d'éval est publié comme brique (D-8.3 : « harnais open
   source »), l'outillage du dépôt suit le régime des briques.
6. **Tests « hello » = invariant de licence** : chaque crate/package teste
   que sa licence suit D-10.1 (`CARGO_PKG_LICENSE`, package.json,
   pyproject.toml). Valide la tuyauterie build+test sans assertion creuse,
   et survivra à la Phase 0.
7. **Format TS par prettier** (le PLAN ne nomme que eslint pour le lint) ;
   eslint flat `strictTypeChecked` + `stylisticTypeChecked` couvre le « pas
   de `any` non justifié ».
8. **commitlint (npm)** + lefthook en devDependencies racine : un seul
   `pnpm install` équipe tout contributeur ; budget hooks < 5 s vérifié
   (mesuré ~1–3 s à cache chaud).
9. **Frontière de langue** : code, identifiants, commentaires de code,
   messages de commit et doc d'API (rustdoc/tsdoc/docstrings) en **anglais**
   (briques destinées à l'écosystème) ; documents de conception (`docs/`),
   gouvernance et commentaires des fichiers de config en **français**.
   `typos` ne vérifie que le code (dictionnaire anglais).
10. **`apps/` reste en placeholders README** jusqu'à leurs phases (cli :
    Phase 2 ; web-client : Phase 5 ; desktop : Phase 7) — la Phase 0 exclut
    tout code fonctionnel ; les workspaces `pnpm`/Cargo les intégreront à
    leur naissance.
11. **uv** : workspace à racine virtuelle `ml/` (data/training/eval),
    backend `uv_build`. Contournement documenté (CONTRIBUTING) pour les
    machines où un driver anti-cheat kernel bloque la création des lanceurs
    `.exe` d'uv ; la CI n'est pas affectée.

## Conséquences

- Un contributeur installe : rustup (lit le toml), corepack/pnpm, uv, typos,
  cargo-deny — tout le reste vient par `pnpm install` / `uv sync`.
- La montée de version des outils (Rust, TS, actions CI) est toujours un
  commit revu, jamais une dérive.
- Dette consignée : aucun formatteur pour YAML/TOML (prettier couvre YAML
  partiellement, TOML pas du tout) — réévaluer si le besoin apparaît.
