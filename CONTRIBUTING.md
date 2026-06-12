# Contribuer à Fluence

Merci de votre intérêt ! Fluence est une plateforme de communication locale et open source
pour le handicap moteur lourd. La qualité du code y est une question de dignité pour ses
utilisateurs : une régression peut littéralement priver quelqu'un de parole.

## Avant de commencer

1. Lisez `docs/SPEC.md` (source de vérité produit & technique — les décisions `D-x.y` sont
   actées) et `docs/PLAN.md` (guide d'exécution : règles §0, pyramide de tests §1, phases §2).
2. La langue de travail des documents est le **français** ; le code, les identifiants et les
   messages de commit sont en **anglais**.

## Règles non négociables (détail : `docs/PLAN.md` §0)

- **Definition of Done** : code + tests du comportement nouveau + doc d'API publique
  (rustdoc/tsdoc/docstring) + CI verte **Windows ET Linux** + self-review. Aucune exception.
- **Bug = test rouge d'abord**, puis le fix — le commit montre rouge→vert.
- **« Le clavier parle toujours »** (SPEC §2.C) : composer et vocaliser ne dépendent jamais
  de la santé des composants IA. Toute PR qui touche le hub préserve les kill-tests.
- **Aucune donnée P0** (conversations, mémoire, voix — SPEC §9.A) dans les logs, les erreurs,
  les fixtures.
- `unsafe` interdit hors crates FFI isolées · clippy `pedantic` · pas de `any` TypeScript
  non justifié · mypy strict.
- Toute décision d'architecture nouvelle = un ADR dans `docs/adr/` (template `0000-template.md`).

## Workflow

- **Trunk-based** : `main` est protégée (CI verte requise, force-push interdit).
- Branches courtes `feat/...`, `fix/...`, `docs/...`, `chore/...` ; PR **squash-mergées**.
- **Conventional commits** (vérifiés par hook et par la CI) :
  `type(scope): description` — types : `feat`, `fix`, `docs`, `test`, `refactor`, `chore`,
  `ci`, `perf`, `build`.

## Mise en place locale

Prérequis : Rust (version épinglée par `rust-toolchain.toml`), Node ≥ 22 (pnpm via corepack),
Python ≥ 3.12 avec [uv](https://docs.astral.sh/uv/), [typos](https://github.com/crate-ci/typos)
et [cargo-deny](https://github.com/EmbarkStudios/cargo-deny).

```bash
corepack enable                  # active pnpm (version épinglée dans package.json)
pnpm install                     # dépendances TS + installe les hooks git (lefthook)
cargo build --workspace          # Rust
uv sync --directory ml           # Python (ml/)
```

Vérifications locales (ce que la CI exécutera) :

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm -r lint && pnpm -r typecheck && pnpm -r build && pnpm -r test
uv run --directory ml ruff check . && uv run --directory ml mypy . && uv run --directory ml pytest
cargo xtask check-licenses       # en-têtes SPDX
cargo deny check                 # licences + advisories des dépendances
```

## Licences des contributions

- `crates/`, `packages/`, `ml/`, `xtask/` : **Apache-2.0** (briques réutilisables, D-10.1).
- `apps/` : **AGPL-3.0-only** (application complète).
- Chaque fichier source porte un en-tête `SPDX-License-Identifier` — vérifié par
  `cargo xtask check-licenses`.

En contribuant, vous acceptez que votre contribution soit publiée sous la licence du
répertoire concerné.

## Sécurité

Ne signalez **jamais** une vulnérabilité par issue publique — voir [SECURITY.md](SECURITY.md).
