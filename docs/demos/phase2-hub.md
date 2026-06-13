# Démo Phase 2 — « le clavier parle toujours »

Cette démo montre les critères « Done quand » de la Phase 2 (PLAN §2) :
appairage depuis un second poste avec `fluencectl`, et résilience du hub
sous des kill répétés d'un worker. Elle est aussi **couverte
mécaniquement** par la CI :

- `apps/cli/tests/cli_against_hub.rs` — pilote `fluencectl` (health,
  pair-window → pair → journal) contre un vrai hub ;
- `crates/fluence-hub/tests/kill_tests.rs` — kill du worker → dégradation
  < 500 ms + relance ; kill -9 du hub en pleine frappe → draft restauré
  (perte ≤ 1 s) ; 50 cycles sans fuite RSS.

## Prérequis

```bash
cargo build --workspace        # hub, worker-echo, fluencectl
```

Les binaires sont sous `target/debug/` : `fluence-hub`, `worker-echo`,
`fluencectl`.

## 1. Lancer le hub (poste principal)

```bash
# Données isolées pour la démo ; clé en fichier (pas de keystore requis).
export FLUENCE_DATA_DIR=/tmp/fluence-demo
export FLUENCE_STORE_KEY_FILE=$FLUENCE_DATA_DIR/store.key
export FLUENCE_ECHO_WORKER=target/debug/worker-echo
target/debug/fluence-hub
```

Le hub écrit `hub.port` et `system.token` dans `FLUENCE_DATA_DIR` et
démarre prêt en < 3 s, worker compris.

## 2. État du hub

```bash
fluencectl --data-dir /tmp/fluence-demo health
# hub 0.0.0 — tier Reduced, up since …
#   worker Unknown: Ready (restarts: 0)
```

## 3. Appairer un second poste

Sur le poste principal, ouvrir une fenêtre d'appairage (2 min, code à
usage unique) :

```bash
fluencectl --data-dir /tmp/fluence-demo pair-window --scope control
#   code: 12345678
```

Sur le **second poste** (qui n'a que l'URL du hub et le code lu à voix
haute), échanger le code contre un jeton :

```bash
fluencectl --url https://hub.local:7411 pair --code 12345678 --name "tablette"
# paired with scope Control; token saved to …/cli-token
```

Le jeton est par appareil, révocable depuis l'espace aidant (Phase 7).

## 4. Suivre les événements

```bash
fluencectl --data-dir /tmp/fluence-demo watch --topics system
# {"topic":"system","msg":{"k":"system.hello",...}}
```

## 5. Résilience : tuer le worker en boucle

Pendant que `watch` tourne, dans un autre terminal :

```bash
# Linux
while true; do pkill -9 -f worker-echo; sleep 1; done
# Windows (PowerShell)
while ($true) { Get-Process worker-echo -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep 1 }
```

`watch` montre la cascade `degraded → down → starting → ready` à chaque
cycle ; le hub reste sain, le compteur de restarts grimpe
(`fluencectl health`), et **rien dans l'entrée n'est jamais bloqué** — le
clavier parlerait toujours.

## 6. Journal d'accès (espace aidant)

```bash
fluencectl --data-dir /tmp/fluence-demo journal --limit 20
# 2026-… pair.window_opened  -  scope=Control
# 2026-… device.paired       …  kind=Cli scope=Control
```

Le journal ne contient que des métadonnées d'accès — jamais de contenu P0
(conversations, drafts, mémoire ; SPEC §9.A).
