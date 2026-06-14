<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# Soak — protocole de tenue longue durée (PLAN 7.6, critère A1)

Le critère A1 (D-12.2) : **72 h de soak — zéro crash, RSS borné, zéro perte de
draft (kills aléatoires inclus)**. Une parole qui s'effondre après deux jours
d'usage n'est pas une parole fiable.

Ce qui est **automatisé** (à chaque CI / nightly) et ce qui exige une **machine
de référence** (FLU-REF) sont distincts et honnêtement séparés ci-dessous — on
ne coche jamais « 72 h » sans l'avoir tenu (PLAN §0.8).

## Invariants vérifiés

1. **Zéro crash** non sollicité : le hub ne meurt que quand on le tue ; il
   redémarre toujours (watchdog superviseur < 2 s, backoff).
2. **RSS borné** : pas de fuite — la mémoire résidente reste stable sur la durée
   (pas de croissance monotone).
3. **Zéro perte de draft** : un `kill -9` en pleine frappe laisse ≤ 1 s de perte
   au redémarrage (D-2.6), mesuré aux timestamps de frappe.

## Proxy automatisé (CI + nightly)

- **À chaque CI** (`cargo test`, deux OS) : `kill_tests`
  - `kill_cycles_keep_rss_bounded` — N cycles kill/respawn du worker supervisé,
    RSS bornée (< +10 % après warm-up). N = `FLUENCE_SOAK_CYCLES` (défaut 50).
  - `hub_killed_mid_typing_draft_restored` — `kill -9` du hub en frappe → draft
    restauré, perte ≤ 1 s.
- **Nightly** (`.github/workflows/nightly.yml`, job `soak`) :
  `FLUENCE_SOAK_CYCLES=300` — un soak étendu, dans la limite des 6 h GitHub.

Honnêteté : le proxy exerce les **cycles de kill** (la voie de défaillance la
plus probable) et la **bornitude RSS**, pas une charge réaliste de 72 h. C'est un
gate de non-régression, pas la preuve A1.

## La preuve A1 : soak 72 h sur FLU-REF (action physique)

À tenir **une fois** sur la machine de référence (FLU-REF-1 ou équivalent
disponible), avant le tag `phase-7-done` :

1. Construire en release : `cargo build --workspace --release`.
2. Lancer le hub avec un data-dir dédié, voix OS + n-gram (pas de modèle lourd
   nécessaire pour le soak) :
   `FLUENCE_DATA_DIR=… FLUENCE_PORT=7411 ./target/release/fluence-hub`.
3. Pilote de charge sur 72 h : un client `control` qui, en boucle,
   ouvre une session, frappe un draft (PUT ~2/s), demande `next-chars`/`suggest`,
   et `PARLE` ; un superviseur externe qui **tue le hub à intervalle aléatoire**
   (`kill -9`, toutes les 10–60 min) et vérifie au redémarrage que le dernier
   draft est intact (perte ≤ 1 s).
4. Échantillonner le RSS toutes les minutes (`ps`/`/proc`), tracer la courbe.
5. **Critères de réussite** : 0 crash non sollicité ; RSS plate (pas de tendance
   haussière sur 72 h) ; 0 perte de draft au-delà du 1 s borné.
6. Consigner : durée réelle, machine, nombre de kills, RSS min/max, courbe, et
   l'attacher au tag.

> Le pilote de charge réutilise exactement les voies de `kill_tests` (binaire
> hub réel, vrai flux d'appairage, kill/respawn) — le proxy CI **est** une
> version courte de ce protocole.

## Seuils `provisional` → `contractual` (FLU-REF)

Les budgets de latence (SPEC §5.A) sont mesurés en `provisional` sur les runners
GitHub (×2,5, PLAN §0.8) et en `contractual` seulement sur FLU-REF. La bascule
exige donc le **runner self-hosted FLU-REF** (action matérielle) ; tant qu'il
n'existe pas, les seuils contractuels restent non vérifiés et c'est documenté,
jamais coché à tort.
