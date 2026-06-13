# ADR-0007 — Intégration llama.cpp et trait `LlmBackend` (Phase 4)

- **Statut** : accepté
- **Date** : 2026-06-13
- **Décisions SPEC liées** : D-2.6 (workers isolés), D-3.1 (backends en cascade), D-3.2 (gestion de modèles), D-5.3 (API du hub, KV chaud), §5.A (moteur)

## Contexte

La Phase 4 du PLAN construit le **moteur** : `rephrase`/`continue` réels,
`next-chars` (distribution du prochain caractère sur le KV chaud), sessions à
cache KV, annulation par slot. Il faut faire tourner un LLM local (llama.cpp,
GGUF) sur CPU, palier 8 Go (FLU-REF-1).

Forces en tension :

1. **Contrôle fin requis** : un KV-cache *par conversation* (D-5.3), l'accès aux
   **logits** pour agréger une distribution de prochains caractères (§5.A
   `next-chars`), la **génération streaming annulable** par slot.
2. **« Le clavier parle toujours » (D-2.6)** : un crash de la bibliothèque native
   (GGML) ne doit jamais tuer le chemin clavier. Les workers tournent donc en
   **processus enfants** supervisés.
3. **`forbid(unsafe_code)`** partout sauf crates FFI isolées (PLAN §0.6).
4. **Windows + Linux**, build reproductible, MSRV épinglée ; le build de
   llama.cpp (C++/CMake) est lourd, surtout en CI à chaque PR.
5. **Cascade de backends (D-3.1)** : local llama.cpp, mais aussi un backend
   distant OpenAI-compatible opt-in plus tard — les deux derrière la même
   abstraction.

## Options considérées

### A. Stratégie de binding llama.cpp

1. **Crate `llama-cpp-2`** (binding Rust sûr au-dessus du `-sys` FFI) — accès
   direct au contexte (KV par session), aux logits (next-chars), génération
   token à token annulable. Coût : build C++/CMake (lourd, à mettre en cache) ;
   l'`unsafe` vit dans le crate `-sys` tiers, notre code reste sûr.
2. **Sous-processus `llama-server`** (serveur HTTP de llama.cpp) — pas de FFI
   chez nous, mais le contrôle du KV par session passe par des « slots » à pool
   fixe, et les logits/`next-chars` sont indirects (logprobs token-level via
   l'API). Un binaire prébuilt à télécharger par plateforme.
3. **FFI maison** — contrôle maximal, coût et risque maximaux. Rejeté.

### B. Où vit l'`unsafe` et l'isolation crash

L'isolation crash (D-2.6) vient du **processus** (`worker-llm` est un enfant
supervisé), pas de l'évitement du FFI. Le FFI in-process est donc acceptable
*dans* ce worker isolé.

## Décision

- **A → option 1 : `llama-cpp-2`.** Le contrôle du KV par session et l'accès aux
  logits pour `next-chars` sont des exigences dures de §5.A/D-5.3 que seul le
  binding in-process sert proprement. L'isolation crash est fournie par la
  frontière de processus (`worker-llm` enfant supervisé — D-2.6), pas par
  l'évitement du FFI ; l'`unsafe` reste confiné au crate `-sys` tiers, notre
  `worker-llm` n'utilise que l'API sûre (`forbid(unsafe_code)` tenu).
- **Trait `LlmBackend`** (dans `fluence-inference`) abstrait le moteur :
  génération streaming annulable (`generate` + `CancelToken` + sink de deltas)
  et distribution de prochains caractères (`next_chars` → `Vec<CharProb>` du
  contrat). Le backend local (llama-cpp-2, Phase 4.2), le backend distant
  OpenAI-compatible (D-3.1) et un **`StubBackend` déterministe** implémentent le
  même trait. Le stub permet de bâtir et tester `fluence-accel` et les endpoints
  (Phase 4.4/4.5) **avant** que le backend réel n'atterrisse.

## Conséquences

- **Plus simple** : une abstraction unique pour local/distant/stub ; le pipeline
  d'accélération se teste sans modèle (stub déterministe) ; KV + logits sous
  contrôle direct.
- **Plus contraint** : le build de llama.cpp (C++/CMake) est lourd → mis en
  cache (Swatinem/rust-cache) et les tests d'intégration LLM tournent dans un
  **job dédié / nightly** (tiny-LLM ~150–300 Mo), pas dans la CI PR rapide ;
  l'`unsafe` du `-sys` est circonscrit, audité via `cargo-deny`.
- **Dette éventuelle** : si le build llama.cpp se révèle instable sur un palier
  Windows, le sous-processus `llama-server` reste un repli — le trait
  `LlmBackend` ne change pas (un `ServerBackend` l'implémenterait).
- **SPEC** : inchangée (D-3.1 prévoyait déjà la cascade ; ceci en précise le
  binding, niveau d'exécution).

## Amendement (2026-06-13) — le sous-processus `llama-server` devient principal

La décision A1 (FFI `llama-cpp-2`) est **rétrogradée en repli** ; le **sous-
processus `llama-server`** (ex-option A3, ex-repli) devient la **réalisation
principale** du backend local.

**Pourquoi (audité)** :
- **Build FFI bloqué sur la machine de dev** : `llama-cpp-sys-2` ne gère pas la
  cible `x86_64-pc-windows-gnu` (msys2/UCRT) — son `build.rs` cherche des `.lib`
  MSVC et trouve 0 lib MinGW (`.a`) → `assert_ne!(libs.len(), 0)` échoue (après
  avoir réglé le faux `#error cpp-httplib` via `-D_WIN32_WINNT=0x0A00`). Non
  compilable/testable en local ; la CI Windows étant MSVC, divergence dangereuse
  pour un backend FFI.
- **`llama-server` n'a aucun de ces défauts** : binaire **officiel** prébuilt
  (indépendant du toolchain Rust), donc compile-free, **testable en local** sur
  windows-gnu (PoC validé : `/health` ok, `/completion` génère), CI portable
  (télécharger binaire + tiny-LLM, pas de compilation C++), **isolation crash
  intrinsèque** (c'est un processus séparé — D-2.6), API complète (streaming
  `/completion`, logprobs `n_probs` → `next-chars`). Risque nul pour les builds
  gnu existants (pur client HTTP Rust).

**Décision** : `LlamaServerBackend` (client HTTP `ureq`, pur Rust, **pas de
feature native**) implémente `LlmBackend` ; le hub spawn/supervise `llama-server`
comme processus enfant et lui parle en HTTP loopback ; la gestion de modèles
(4.3) télécharge le binaire + le tiny-LLM (manifeste + sha256). Le FFI
`llama-cpp-2` reste une optimisation future (contrôle KV plus fin) si jamais le
HTTP loopback devient un goulot — le trait `LlmBackend` ne change pas.

**Conséquences** : aucune compilation C++ dans notre build/CI (gain de fiabilité
et de portabilité majeur) ; un binaire + un modèle à télécharger et vérifier
(4.3) ; le surcoût HTTP loopback est négligeable devant les budgets §5.A.
