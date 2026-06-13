# ADR-0004 — Décisions de contrat v1 (Phase 1)

- **Statut** : accepté
- **Date** : 2026-06-13
- **Décisions SPEC liées** : D-2.4, D-2.5, D-4.1, D-5.3 ; PLAN tâches 1.1–1.3bis

## Contexte

La SPEC définit les messages et endpoints (§2.A, §4.A, §5.A, §5.B) mais laisse
ouverts des détails de contrat que la Phase 1 doit trancher pour générer des
artefacts complets. Chaque choix ci-dessous est un détail d'exécution fidèle à
l'esprit de la SPEC ; aucun ne la contredit.

## Décisions

1. **Abonnement WebSocket par query string** (`GET /ws?topics=…&v=1`), réponse
   = première frame `system.hello` (topics accordés ∩ scope, version retenue).
   Évite un aller-retour de souscription avant les premiers événements ; le
   « champ v:1 négocié à l'ouverture » (§4.A) est réalisé à l'ouverture même.
   Changement d'abonnement = reconnexion (rare : la reprise §2.A est déjà
   prévue). *Amendé (PR SDK)* : le **token passe aussi en query param**
   (`&token=…`) pour le WS — l'API `WebSocket` des navigateurs ne peut pas
   poser le header `X-Fluence-Token`. Surface acceptée : loopback sans TLS
   ou TLS local ; le hub ne journalise jamais la query string de `/ws`.
2. **Topics réservés sans payload v1** : `asr`, `suggest`, `voice` sont des
   noms d'abonnement valides mais aucune frame n'y circule avant P2 — leurs
   messages seront spécifiés avec leurs phases (8+). L'enveloppe
   (`{"topic", "msg"}`) et `ServerFrame` sont `non_exhaustive`.
3. **Politique de tolérance différenciée** :
   - enums d'événements serveur→client : `#[non_exhaustive]` (ajout non
     cassant ; les clients ignorent l'inconnu) ;
   - enums de présentation (`TargetRole`, `CommitMethod`, `DeviceKind`,
     `WorkerKind`, `SuggestionOrigin`, `ErrorCode`…) : variante
     `#[serde(other)] Unknown` (un pair plus récent ne casse pas le parse) ;
   - enums de **sécurité ou de sens** (`Scope`, `Speaker`, `TurnSource`,
     `MemoryAcl`) : **fail closed** — une valeur inconnue est rejetée.
4. **Interprétation de « `#[non_exhaustive]` côté Rust » (PLAN 1.3bis)** :
   appliqué aux enums des domaines experimental ; les structs experimental
   restent constructibles (le droit de casser est porté par le niveau de
   stabilité documenté + `x-fluence-stability`, pas par une contrainte de
   construction qui pénaliserait le hub).
5. **Types de problème RFC 9457 en URN** : `urn:fluence:problem:<code>` —
   stable sans posséder de domaine (D-12.4 : domaines à vérifier avant
   publication) ; pourra devenir une URL résolvable sans changer les codes.
   Le catalogue machine vit dans le champ extension `code`.
6. **Goldens** : `schemas/<Type>.json` autonomes (`$defs` inline, diffs
   revuables) + `schemas/openapi.json` assemblé depuis le **registre de
   routes déclaratif** (`fluence_protocol::routes()` — la surface API comme
   données testées : unicité, préfixe `/api/v1`, stabilité par domaine,
   routes sans token limitées à l'appairage). Le hub (Phase 2) s'alignera
   sur ce même registre.
7. **`serde_json/float_roundtrip`** : round-trip f64 exact au bit près —
   découvert par proptest (1 ULP de dérive sans la feature) ; le replay des
   traces (T4) et le déterminisme d'éval (Phase 3) l'exigent.
8. **Fichier TS généré préfixé `SPDX-License-Identifier`** par le générateur :
   aucun cas spécial dans `check-licenses`. Les artefacts générés sont exclus
   de prettier/eslint (leur forme canonique est celle du générateur,
   vérifiée par `--check`) mais couverts par `tsc` (T3 : « le SDK généré
   compile »).
9. **Détails wire ajoutés** (non spécifiés par la SPEC, marqués dans la doc
   des types) : patch incrémental de cibles (`upsert` puis `remove`) ;
   `scope` accordé dans `PairResponse` ; `POST /asr/consent` (le
   `consent_token` exigé par §5.A doit bien être délivré quelque part —
   experimental, flux précisé en Phase 8) ; flux d'oubli en deux temps
   (candidats → `DELETE` confirmés, §5.B).

## Conséquences

- La dérive de contrat est une erreur de CI (`check-contracts --check`,
  prouvé par mutation) ; les diffs de goldens sont lisibles en review.
- Les clients écrits contre le SDK tolèrent les hubs plus récents (politique
  3) ; la sécurité ne tolère rien.
- Dette assumée : les messages WS `asr`/`suggest`/`voice` restent à spécifier
  (P2) ; le flux de consentement ASR sera revu en Phase 8 (experimental).
