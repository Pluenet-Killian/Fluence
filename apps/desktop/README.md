<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# apps/desktop — application de bureau Tauri v2 (Phase 7.1)

Application double-clic qui **embarque et supervise le hub** (autostart + watchdog
< 2 s) et ouvre une webview sur le composeur servi par le hub. Conformément à
**D-2.1**, l'UI ne parle au hub **que via l'API réseau locale** (`http://127.0.0.1:7411`)
— un seul chemin de code, le mode déporté est gratuit.

Licence : AGPL-3.0-only (application complète, D-10.1).

## État de livraison (honnête, PLAN §0.8)

| Brique | Statut |
|---|---|
| **Watchdog du hub** (`crates/fluence-watchdog`) | ✅ **livré et testé** (spawn + surveille + redémarre < 2 s, backoff plafonné, autostart) — le cœur automatisable de 7.1 |
| Coquille Tauri (webview → API locale) | ✅ projet `src-tauri/` livré (Cargo.toml + `main.rs` qui supervise le hub via `fluence-watchdog` + `tauri.conf.json`) — **exclu du workspace** (build par la toolchain Tauri, hors CI par-PR : WebView2/WebKitGTK requis) |
| Installeurs MSI/NSIS (Windows), AppImage/deb (Linux) | ✅ pipeline livré : workflow `release` (dispatch manuel) build le sidecar hub + génère les icônes + `cargo tauri build` ; à valider/finaliser au 1ᵉʳ release réel |
| **Signature** des installeurs (Authenticode / GPG / notarisation) | ⛔ **gate credential** : nécessite un **certificat de release** (secret d'opérateur, jamais dans ce dépôt) ; le workflow signe **quand le secret est présent**, sinon non signé |

Le watchdog est vérifié mécaniquement (tests cross-OS). La coquille Tauri compile
avec la toolchain Tauri (le projet `src-tauri` est écrit sur l'API v2 et validé au
build toolchain, non en CI par-PR). La **signature** exige un certificat — la case
« installable » reste non cochée tant qu'un installeur n'est pas réellement signé
**et** installé chronométré par un tiers (< 30 min, critère A1, PLAN §0.8).

## Architecture

```
┌────────────────────────── Application Tauri ──────────────────────────┐
│  src-tauri (Rust)                                                      │
│    fluence_watchdog::Watchdog::spawn(hub binaire, FLUENCE_WEB_DIR=…)   │
│      └─ relance le hub < 2 s s'il meurt (autostart + auto-réparation)  │
│  webview ──────────────► http://127.0.0.1:7411  (PWA servie par le hub)│
└───────────────────────────────────────────────────────────────────────┘
```

Le hub est lancé comme **sidecar** (binaire `fluence-hub` embarqué dans le bundle
Tauri), surveillé par `fluence-watchdog`. Si le hub tombe, le watchdog le relance
sous la barre des 2 s ; la webview se reconnecte (le composeur a déjà sa logique
de reconnexion WS, et le brouillon est restauré — perte ≤ 1 s, D-2.6).

## Le projet `src-tauri`

`src-tauri/src/main.rs` supervise le hub via `crates/fluence-watchdog` (std pur,
testé en CI sur les deux OS — la logique de supervision est prouvée indépendamment
de l'enrobage graphique) puis lance la webview pointée sur l'API locale :

```rust
let _hub = spawn_hub();                 // sidecar fluence-hub sous watchdog (< 2 s)
tauri::Builder::default()
    .run(tauri::generate_context!())     // webview → http://127.0.0.1:7411
    .expect("error while running the Fluence desktop app");
```

Le crate est **exclu du workspace** (`Cargo.toml` racine) : `cargo build --workspace`
et la CI par-PR ne le compilent pas (pas de WebView2/WebKitGTK requis). Il se
construit avec la toolchain Tauri.

## Construire les installeurs

En local (toolchain Tauri installée : Rust + Node, WebView2 sur Windows /
WebKitGTK + libs de build sur Linux ; `cargo install tauri-cli --version ^2`) :

```bash
pnpm --filter @fluence/web-client build                 # la PWA servie par le hub
cargo build --release -p fluence-hub                     # le sidecar
triple=$(rustc -vV | sed -n 's/host: //p')               # ex. x86_64-pc-windows-msvc
mkdir -p apps/desktop/src-tauri/binaries
cp target/release/fluence-hub* "apps/desktop/src-tauri/binaries/fluence-hub-$triple"  # (.exe sur Windows)
cd apps/desktop/src-tauri
cargo tauri icon icons/source.png        # génère le jeu d'icônes (fournir un PNG source)
cargo tauri build                        # → MSI/NSIS (Windows) ou AppImage/deb (Linux)
```

En CI : le workflow **`release`** (dispatch manuel) fait tout ceci (job `desktop`),
génère un placeholder d'icône, et **signe quand le certificat est configuré**.

## Signer (étape opérateur, gate credential)

Le **certificat de signature** est un secret d'opérateur, **jamais** dans ce dépôt.

- **Windows (Authenticode)** : déposer le PFX (base64) dans le secret
  `WINDOWS_CERTIFICATE_BASE64` et son mot de passe dans `WINDOWS_CERTIFICATE_PASSWORD`.
  Le job `release` importe le certificat, lit son empreinte et la passe à
  `cargo tauri build --config '{"bundle":{"windows":{"certificateThumbprint":"…"}}}'`.
  Les paramètres `digestAlgorithm`/`timestampUrl` sont déjà dans `tauri.conf.json`.
  Sans le secret → build **non signé** (smoke test uniquement).
- **Linux** : signer les paquets `deb` (GPG) / l'AppImage selon ta clé.

Tant qu'aucun certificat réel n'a signé un installeur **et** qu'un tiers ne l'a
pas installé chronométré (< 30 min, critère A1), la case « installable » reste
non cochée (PLAN §0.8). Remplacer aussi `icons/source.png` par la vraie identité
visuelle avant une release publique.
