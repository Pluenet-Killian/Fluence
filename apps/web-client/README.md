<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# `@fluence/web-client` — composeur web (PWA)

La première expérience utilisable de Fluence (PLAN 5.3, SPEC §7.A) : un clavier à
**dwell** (souris d'abord), trois emplacements de suggestions **fixes**, un
bouton **PARLER** invariant et une **urgence** à double confirmation, le tout
câblé au hub via `@fluence/sdk` (WebSocket pour la sélection, SSE pour les
suggestions, REST pour la voix et l'urgence). Licence : AGPL-3.0-only (D-10.1).

## Architecture

- **Vanilla TS + Vite** : pas de framework, contrôle DOM direct (jauges de
  dwell, règles anti-scintillement), bundle minuscule, servi en statique.
- **Logique pure, testée** (`vitest`) : `coords` (normalisation pointeur),
  `antiflicker` (`SuggestionGate`, SPEC §7.A : 1 maj/600 ms, jamais pendant un
  dwell > 40 %), `keyboard` (layout + `TargetMap`).
- **`composer.ts`** orchestre : session, déclaration des cibles
  (`PUT /input/targets`), socket (`sel.*` → frappe + jauge ; `system.*` →
  bannière d'urgence / mode dégradé), streaming des échantillons pointeur vers le
  hub (le hit-testing + dwell vivent dans le hub, D-4.1), suggestions SSE,
  `PARLER` (lecture du WAV), urgence (`client.emergency`), reconnexion.
- **Deux voies de frappe** : clic direct (universel, testable) **et** dwell
  hub-side (la voie accessible). Les deux passent par un seul `type()`.
- **i18n dès v0** (`i18n.ts`) : toutes les chaînes sont des clés (FR seul fourni,
  SPEC §1.4). **Contraste élevé** (`styles.css`).

## Développement

```bash
pnpm --filter @fluence/web-client dev        # Vite + proxy du hub (127.0.0.1:7411)
pnpm --filter @fluence/web-client typecheck
pnpm --filter @fluence/web-client lint
pnpm --filter @fluence/web-client test        # tests unitaires (logique pure)
pnpm --filter @fluence/web-client build        # → dist/
```

En dev, Vite **proxifie** `/api`, `/ws`, `/pair` vers le hub (même origine côté
navigateur, pas de CORS). En production, le **hub sert** `dist/` en statique
(même origine) : lancer le hub avec `FLUENCE_WEB_DIR=<…>/apps/web-client/dist`.

Un jeton `control` est demandé au premier lancement (collez-en un appairé via
`fluencectl` ou l'écran principal) ; il est conservé en `localStorage`.

## Boucle complète (démo / e2e)

La boucle complète (dwell → composer → suggestions → PARLER) nécessite un hub
**intégré** : `fluence-input` (sélection, PR #38), `worker-tts` (voix, PR #39) et
l'urgence (PR #37), en plus du moteur (#35) et d'un modèle. Tant que ces
branches ne sont pas mergées sur `main`, lancer la démo demande un hub assemblé.

La **suite Playwright T5** (scénarios personas — « Marc compose “bonjour” au
dwell et le fait parler », acceptation d'une suggestion, urgence à double
confirmation reçue par un 2ᵉ client, reconnexion à chaud) est le **suivi
d'intégration** : elle se branche en CI (`integration.yml`) une fois le hub
intégré disponible + `pnpm exec playwright install`. Dette suivie.
