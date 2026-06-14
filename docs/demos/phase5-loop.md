# Démo Phase 5 — la boucle complète

Cette démo montre le critère « Done quand » de la Phase 5 (PLAN §2) : **composer
une phrase au dwell, accepter une suggestion, PARLER en Piper FR, déclencher puis
annuler une urgence**, dans le vrai composeur web servi par le hub assemblé. Elle
est **reproductible** (un script Playwright la rejoue et l'enregistre en vidéo) et
**couverte mécaniquement** par la suite T5 en CI :

- `apps/e2e/src/specs/*.spec.ts` — les quatre scénarios persona (dwell + PARLER +
  autosave, suggestion acceptée, urgence double-confirmation reçue par un 2ᵉ
  client, hub tué → reconnexion + draft intact), exécutés sur Windows **et** Linux
  par `.github/workflows/integration.yml` (job `e2e`).

La suite T5 tourne sans modèle lourd (suggestions par le **fallback n-gram**, voix
par la **voix OS** — espeak-ng sur Linux, SAPI sur Windows). La démo ci-dessous
utilise en plus **Piper** quand il est provisionné, pour entendre la voix FR.

## Prérequis

```bash
cargo build -p fluence-hub                       # le binaire du hub assemblé
pnpm --filter @fluence/web-client build          # la PWA que le hub sert
pnpm --filter @fluence/e2e exec playwright install chromium
```

## Voix Piper (optionnelle, pour « PARLER en Piper FR »)

Le harnais transmet `FLUENCE_PIPER_BIN`/`FLUENCE_PIPER_VOICE` au hub s'ils sont
exportés ; sinon la voix OS répond (« une voix, toujours », SPEC §2.C). Avec le
Piper déjà provisionné sous `.fluence-cache/` :

```bash
# Windows (PowerShell)
$env:FLUENCE_PIPER_BIN   = "$PWD\.fluence-cache\piper\piper\piper.exe"
$env:FLUENCE_PIPER_VOICE = "$PWD\.fluence-cache\voices\fr_FR-siwis-medium.onnx"

# Linux / macOS
export FLUENCE_PIPER_BIN="$PWD/.fluence-cache/piper/piper/piper"
export FLUENCE_PIPER_VOICE="$PWD/.fluence-cache/voices/fr_FR-siwis-medium.onnx"
```

## Lancer la démo

```bash
pnpm --filter @fluence/e2e demo
```

Le script (`src/demo/loop.demo.ts`) ouvre le composeur en fenêtre visible, fait
défiler la boucle avec des pauses lisibles, et **enregistre une vidéo**. Aucune
configuration de hub à la main : le harnais (`src/hub-harness.ts`) lance un hub
jetable sur un port loopback libre, sert la PWA buildée, et appaire un jeton
`control` par le vrai flux d'appairage — exactement comme un appareil réel.

## Le film

La vidéo est écrite sous :

```
apps/e2e/demo-output/<nom-du-test>/video.webm
```

C'est l'artefact « démo filmée » du critère Done-quand. (Le dossier
`demo-output/` est git-ignoré : on filme, on ne versionne pas le binaire vidéo.)

## Ce que la démo prouve

1. **Dwell-souris** : le pointeur stationne sur une touche, le hub accumule le
   dwell (800 ms) et émet un `sel.commit` que le composeur transforme en frappe.
2. **Accélération** : une suggestion proposée est acceptée et remplace le draft.
3. **Voix** : PARLER renvoie un WAV non vide (Piper FR si provisionné, voix OS
   sinon) — jamais un 200 silencieux.
4. **Urgence** : la double confirmation protège du déclenchement accidentel
   (armement → annulation), puis la confirmation diffuse la bannière à un second
   client appairé (D-7.4, SPEC §7.A).
```
