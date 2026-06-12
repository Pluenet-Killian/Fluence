# Fluence

Plateforme de communication open source, **locale et intégrée**, pour le handicap moteur lourd (SLA en tête) : entrée multimodale (regard/tête/contacteur), accélération de la communication par LLM local, voix personnelle. Objectif : **×3 sur les mots/minute effectifs en conversation**, 100 % offline, données au foyer.

## Documents de référence — à lire avant de coder

1. **`docs/SPEC.md`** — la source de vérité produit & technique. Toutes les décisions sont numérotées `D-x.y` et actées ; les specs détaillées sont en sous-sections (§2.A, §4.A, §5.A…). *Rien ne se code qui contredise la SPEC ; si la réalité la contredit, on l'amende explicitement (jamais en silence).*
2. **`docs/PLAN.md`** — le guide d'exécution : règles d'exécution (§0), pyramide de tests T1–T6 (§1), phases 0–7 jusqu'au jalon A1 (§2), état d'avancement (§5).

## Rituel de session — obligatoire

- **Début** : lire `docs/PLAN.md` §5 (état) → ouvrir la phase courante → exécuter ses tâches dans l'ordre.
- **Fin** : mettre à jour PLAN §5, cocher les critères « Done quand » atteints, noter la dette en issues `debt`, consigner toute déviation. *Une session qui ne met pas à jour l'état n'a pas terminé.*

## Règles non négociables (détail : PLAN §0)

- **Definition of Done** : code + tests du comportement nouveau + doc API publique + CI verte **Windows ET Linux** + self-review. Aucune exception « c'est trivial ».
- **Bug = test rouge d'abord**, puis le fix (le commit montre rouge→vert).
- **« Le clavier parle toujours »** (SPEC §2.C) : composer et vocaliser ne dépendent jamais de la santé des composants IA. Toute PR qui touche le hub préserve les kill-tests.
- **Aucune donnée P0** (conversations, mémoire, voix — SPEC §9.A) **dans les logs, les erreurs, les fixtures**.
- `unsafe` interdit hors crates FFI isolées ; clippy pedantic ; pas de `any` TS non justifié ; mypy strict.
- Toute décision d'architecture en cours de route = **ADR** dans `docs/adr/` ; conventional commits ; trunk-based, PR squash sur `main` protégée.
- Latences : seuils `provisional` (CI GitHub) vs `contractual` (machines FLU-REF, SPEC §5.A) — ne jamais « ajuster » un seuil contractuel pour faire passer un build.

## Vocabulaire

`Phase 0–7` = étapes d'exécution (PLAN) · `P1/P2/P3` = phases produit en mois (SPEC §12) · `A1/B1/1.0` = jalons (D-12.2) · `D-x.y` = décisions (SPEC, Annexe A) · personas : **Claire** (SLA, tracker IR), **Marc** (locked-in, webcam seule, 8 Go), **Sophie** (ergothérapeute, 30 min max), **Jean** (aidant non technophile).

## Structure (détail : SPEC §2.B)

`crates/` Rust (hub, protocol ★source de vérité des schémas, inference, input, accel, voice, store) · `apps/` (desktop Tauri, web-client, cli) · `packages/` TS (sdk, ui, integrations) · `ml/` Python (data, training, eval) · `models/` manifestes · `docs/` (SPEC, PLAN, adr/).

## Commandes

```bash
# Rust
cargo build --workspace && cargo test --workspace
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
cargo xtask check-licenses          # en-têtes SPDX + architecture (D-10.1)
cargo xtask check-contracts         # disponible en Phase 1 (exit 2 avant)
cargo xtask run-eval --suite pr     # disponible en Phase 3 (exit 2 avant)
cargo deny check                    # licences + advisories des dépendances

# TypeScript (pnpm via corepack ; `pnpm install` installe aussi les hooks git)
pnpm -r lint && pnpm -r typecheck && pnpm -r build && pnpm -r test
pnpm format:check                   # prettier

# Python (ml/)
uv sync --directory ml
uv run --directory ml ruff check . && uv run --directory ml ruff format --check .
uv run --directory ml mypy . && uv run --directory ml pytest

# Lancement du hub en dev : Phase 2+.
# Windows + anti-cheat kernel (lanceurs uv bloqués) : voir CONTRIBUTING.md.
```
