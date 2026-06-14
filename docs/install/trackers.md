<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# Matrice de compatibilité — pointage & trackers v0 (PLAN 7.5)

Fluence sépare le **pointage** (d'où vient le curseur) de la **sélection** (dwell
hub-side). N'importe quelle source qui bouge un point sur la surface peut piloter
le clavier ; les sources de regard brut passent en plus par la calibration et la
fusion (SPEC §4.C). Convention de routage par préfixe d'identifiant de source :

- `mouse:…` → **dwell direct** (la source est déjà un curseur fiable) ;
- `gaze:…` → **calibration + fusion + I-VT + magnétisme** puis dwell.

## Niveaux d'autonomie

| Niveau | Définition | Statut |
|---|---|---|
| **0** | Pointage fiable type souris (le tracker émule la souris OS, ou souris/écran tactile) | **Testé** (suite e2e T5, deux OS) |
| **1** | Regard brut webcam (MediaPipe) → calibration express → composition | **Moteur + client livrés et testés** ; session webcam réelle = action physique (→ `phase-6-done`) |
| **2** | ML-regard dédié, multi-caméra, contacteur scanning avancé | Post-A1 (conception, SPEC §4.C / 6.5) |

## Matrice par modalité

| Modalité | Voie | Niveau | Statut v0 |
|---|---|---|---|
| Souris / pavé / écran tactile | `mouse:` → dwell | 0 | ✅ testé (dwell + PARLER + autosave en e2e) |
| **Tracker IR dédié émulant la souris** (ex. persona Claire) | `mouse:` → dwell | 0 | ✅ supporté via la souris OS (aucune intégration spécifique requise) |
| **Webcam (regard)** — MediaPipe Face Landmarker | `gaze:webcam` → calibration/fusion | 1 | 🟡 moteur (One Euro, I-VT, fusion, magnétisme, calibration ridge) + client livrés/testés ; **session réelle à démontrer** |
| **Pose de tête** (affine le regard) | fusion `head_affine` (zone bornée) | 1 | 🟡 livré dans le moteur (borné à la zone) ; exploité avec le regard |
| **Contacteur / switch** | `on_switch` (voie moteur) | 1→2 | 🟡 voie câblée dans le moteur ; UI de **scanning** = à venir |
| Tracker exposant un regard brut via une source dédiée | `gaze:<id>` → calibration/fusion | 1 | 🟡 même pipeline que la webcam (fournir les features de regard normalisées) |

## Notes d'intégration

- **Tracker qui émule la souris** : rien à faire — il pilote le `mouse:` dwell
  (niveau 0). C'est le chemin le plus robuste pour une mise en route rapide.
- **Tailles de cible adaptatives** : le modèle de bruit par utilisateur agrandit
  les cibles d'un utilisateur plus dispersé (SPEC §4.C) ; la **clause de pivot**
  (PLAN §6) assume des cibles plus grandes + fusion tête si la précision réelle <
  80 %, documenté plutôt que survendu.
- **Honnêteté** : « testé » = couvert mécaniquement (e2e ou property-tests) ;
  « livré/testé, session réelle à démontrer » = le code et ses tests existent, la
  démonstration sur matériel réel est une action physique non encore cochée
  (PLAN §0.8). La grille s'étoffera au contact de vrais trackers (dette suivie).
