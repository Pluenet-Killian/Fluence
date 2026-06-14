# Phase 6 — Le regard webcam (mise en route)

Le composeur sait piloter le **regard webcam** (SPEC §4) : MediaPipe Face
Landmarker estime le regard, le hub le **calibre** (régression ridge par profil)
et le fusionne, puis le dwell sélectionne — exactement le pipeline `fluence-input`
testé. Le regard est **opt-in** : par défaut le composeur reste à la souris/dwell
(et la suite e2e T5 n'y touche pas).

## Provisionnement offline — **une commande**

Le projet est **100 % offline** (SPEC §1) : le WASM MediaPipe et le modèle
`face_landmarker.task` se chargent depuis des chemins **locaux**, jamais un CDN.
Après `pnpm install` (qui apporte le WASM dans `node_modules`) :

```bash
cargo xtask download-gaze-assets        # --check valide le manifeste sans réseau
```

Cela télécharge le modèle (**sha256-vérifié**, manifeste `models/gaze-assets.json`,
même contrat d'intégrité que les modèles de test) et copie le WASM, dans :

```
apps/web-client/public/
  mediapipe/wasm/               # @mediapipe/tasks-vision WASM (vision_wasm_*.{js,wasm})
  models/face_landmarker.task   # le modèle face-landmarker (≈ 3,8 Mo)
```

Ces dossiers sont git-ignorés (`apps/web-client/.gitignore`) ; les chemins runtime
sont configurables (`GazeSourceOptions.wasmPath` / `modelPath`).

## Utiliser le regard

1. Build + hub servant la PWA (cf. `docs/demos/phase5-loop.md`).
2. Dans le composeur : **« Regard »** → autorise la caméra et démarre le suivi
   (le regard arrive au hub en `ptr` source `gaze:webcam`).
3. **« Calibrer »** : fixer chaque touche surlignée tour à tour (calibration
   express) ; le hub ajuste le mapping (`cal.sample` puis `cal.fit`).
4. Composer au regard : le dwell sélectionne la touche fixée ; PARLER vocalise.

Sans calibration, le hub ne peut pas mapper le regard → il met le dwell en pause
(rien n'est sélectionné par erreur). L'entrée ne dépend **jamais** de la caméra :
en cas d'échec (caméra refusée, modèle absent), le composeur garde la souris/dwell.

## `record-gaze` — capturer une vérité terrain (6.4)

**« Enregistrer le regard »** rejoue la même séquence de fixations mais **capture**
les paires (regard brut → cible) dans un `GazeSession` (format du replay Rust),
téléchargé en JSON. C'est de la **vraie** donnée : la passer à
`cargo xtask gaze-accuracy` (ou `fluence_input::evaluate`) mesure la précision
réelle — distincte des datasets synthétiques du gate de non-régression (clause de
pivot §6 : si < 80 % de sélections correctes, on assume des cibles plus grandes +
fusion tête, documenté honnêtement plutôt que de mentir sur la précision).

## Pour `phase-6-done`

Reste une **session webcam réelle** (humain + caméra) démontrant calibration 45 s
puis composition au regard seul, sur FLU-REF-4 ou équivalent — action physique
hors automatisation. Le code (moteur + hub + client) est en place.
