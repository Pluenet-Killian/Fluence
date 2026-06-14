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
| Coquille Tauri (webview → API locale) | 🟡 scaffold (`tauri.conf.json`) ; le `src-tauri` se construit avec la toolchain Tauri (hors CI : WebView2/WebKitGTK requis) |
| Installeurs MSI/NSIS (Windows), AppImage/deb (Linux) | 🟡 cibles déclarées dans `tauri.conf.json` ; `tauri build` les produit |
| **Signature** des installeurs (Authenticode / GPG / notarisation) | ⛔ **gate credential** : nécessite des **certificats de release** (secret d'opérateur, jamais dans ce dépôt) |

Le watchdog est la partie vérifiable mécaniquement (tests cross-OS) ; la coquille
et les installeurs exigent la toolchain Tauri, et la **signature** exige des
certificats — non cochés tant qu'ils ne sont pas réellement produits et signés.

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

## Intégration du watchdog (exemple)

Dans `src-tauri/src/main.rs` (à construire avec la toolchain Tauri) :

```rust
use fluence_watchdog::{Watchdog, WatchdogConfig};

fn main() {
    // Le binaire hub est embarqué comme sidecar Tauri ; on le résout puis on
    // le supervise. La webview (tauri.conf.json) pointe sur l'API locale.
    let hub = resolve_sidecar_path("fluence-hub");
    let _watchdog = Watchdog::spawn(
        WatchdogConfig::new(hub).env("FLUENCE_WEB_DIR", web_dir()),
    );
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running the Fluence desktop app");
    // `_watchdog` drop à la sortie → arrête proprement le hub.
}
```

`crates/fluence-watchdog` est volontairement hors de la toolchain Tauri (std pur,
testé en CI sur les deux OS) : la logique de supervision est prouvée
indépendamment de l'enrobage graphique.

## Construire les installeurs

```bash
pnpm --filter @fluence/web-client build      # la PWA servie par le hub
cargo build --release -p fluence-hub          # le sidecar
# Puis, avec la toolchain Tauri installée :
cargo tauri build                             # → MSI/NSIS (Windows) ou AppImage/deb (Linux)
```

Pré-requis toolchain : Rust + Node, et selon l'OS WebView2 (Windows) /
WebKitGTK + librairies de build (Linux). Voir la doc Tauri v2.

## Signer (étape opérateur, gate credential)

La **clé/le certificat de signature** est un secret d'opérateur, **jamais** dans
ce dépôt :

- **Windows** : Authenticode — `tauri.conf.json` → `bundle.windows.certificateThumbprint`
  (ou variables d'environnement de signature) avec un certificat EV/OV.
- **Linux** : signature GPG des paquets `deb` ; AppImage signé.

Tant qu'aucun certificat réel n'a signé un installeur **et** qu'un tiers ne l'a
pas installé chronométré (< 30 min, critère A1), la case « installable » reste
non cochée (PLAN §0.8).
