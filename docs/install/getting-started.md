<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# Installer et lancer Fluence (Windows / Linux)

Guide pas-à-pas (PLAN 7.5, critère A1 « installation < 30 min par un tiers »).
Fluence est **100 % local** : aucune donnée ne quitte le foyer (SPEC §1).

> **État A1** : l'application double-clic (Tauri, qui embarquera et surveillera le
> hub) arrive en 7.1. En attendant, ce guide décrit l'installation **depuis les
> binaires / les sources**, qui fonctionne aujourd'hui de bout en bout. Une fois
> l'installeur signé livré, l'étape 1 deviendra « double-cliquer l'installeur » ;
> tout le reste (appairage, voix, espace aidant) est identique.

## Aperçu en 1 minute

Fluence a un **hub** (le cœur toujours vivant : clavier, voix, suggestions) et
des **clients** (le composeur web, l'espace aidant) que le hub sert. On lance le
hub, on appaire un appareil, on compose.

## 1. Préparer la machine

| | Windows | Linux |
|---|---|---|
| Rust (build) | [rustup](https://rustup.rs) | rustup |
| Node + pnpm (PWA) | Node 22 + `corepack enable` | idem |
| Voix OS (repli, toujours dispo) | SAPI (intégré) | `sudo apt install espeak-ng` |

> Build interrompu par un anti-triche noyau (Vanguard…) ? Voir `CONTRIBUTING.md`.

## 2. Construire le hub et le composeur

```bash
cargo build --release -p fluence-hub          # le binaire du hub
pnpm install && pnpm --filter @fluence/web-client build   # la PWA (dans dist/)
```

## 3. Lancer le hub

Le hub écoute en **loopback** (`127.0.0.1:7411`) par défaut et sert la PWA quand
on lui indique le dossier `dist/` :

```bash
# Windows (PowerShell)
$env:FLUENCE_WEB_DIR="apps/web-client/dist"; ./target/release/fluence-hub
# Linux
FLUENCE_WEB_DIR=apps/web-client/dist ./target/release/fluence-hub
```

Au premier lancement le hub crée son **store chiffré** et sa **clé** dans le
dossier de données de l'OS (`%APPDATA%/fluence` / `~/.local/share/fluence`), et
écrit un **jeton système** (`system.token`) et le port réel (`hub.port`) à côté.
Ouvrez ensuite **http://127.0.0.1:7411** dans un navigateur.

## 4. Appairer l'appareil de la personne (scope « control »)

Depuis la machine du hub (le jeton système y est lisible), avec `fluencectl` :

```bash
cargo run --release -p fluencectl -- pair-window --scope control   # → un code à 8 chiffres (2 min)
```

Dans le composeur web : entrez le code… ou plus simplement collez un jeton
`control` obtenu via :

```bash
cargo run --release -p fluencectl -- pair --code <CODE> --name "Tablette du lit"
```

Le composeur garde le jeton localement. Composez au **dwell** (survol/regard),
acceptez une **suggestion**, appuyez **PARLER**. (Démo détaillée :
`docs/demos/phase5-loop.md`.)

## 5. Voix française (Piper, optionnel mais recommandé)

Sans configuration, la **voix OS** parle déjà (« une voix, toujours », SPEC §2.C).
Pour la voix FR Piper, indiquez le binaire + le modèle au hub :

```bash
FLUENCE_PIPER_BIN=/chemin/piper FLUENCE_PIPER_VOICE=/chemin/fr_FR-siwis-medium.onnx \
FLUENCE_WEB_DIR=apps/web-client/dist ./target/release/fluence-hub
```

## 6. Accélération par LLM (optionnel)

Sans modèle, les suggestions viennent du **repli n-gram** français (jamais vide).
Pour l'accélération neuronale, fournissez `llama-server` + un modèle GGUF :

```bash
cargo run -p xtask -- download-test-assets    # modèle de test (mécanique, pas qualité)
FLUENCE_LLAMA_SERVER_BIN=/chemin/llama-server FLUENCE_LLAMA_MODEL=/chemin/modele.gguf \
FLUENCE_WEB_DIR=apps/web-client/dist ./target/release/fluence-hub
```

## 7. Regard webcam (optionnel)

Provisionner les assets MediaPipe offline et activer **Regard** dans le composeur :
voir `docs/demos/phase6-gaze.md`.

## 8. Espace aidant

Appairez un jeton **`care`** (`pair-window --scope care`), puis ouvrez
**http://127.0.0.1:7411/#care** : santé du système, appareils appairés (avec
révocation), journal d'accès. L'aidant n'accède jamais au contenu des
conversations (SPEC §7.C).

## 9. Sauvegarder (et restaurer) ses données

**Hub arrêté**, créez une sauvegarde chiffrée + son **kit de secours** imprimable :

```bash
./target/release/fluence-hub backup --out fluence.backup
# → fluence.backup + fluence.backup.kit.svg + une PHRASE à imprimer/garder
```

Restaurer sur une autre machine (avec le kit) :

```bash
./target/release/fluence-hub restore --in fluence.backup --recovery "<phrase du kit>"
```

Tout effacer (oubli, SPEC §9.A) : `fluence-hub wipe --yes`.

## Et après ?

- Ça ne marche plus ? → `docs/install/troubleshooting.md` (1 page).
- Quel matériel de pointage ? → `docs/install/trackers.md`.
