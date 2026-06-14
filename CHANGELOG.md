# Changelog

Toutes les évolutions notables du projet, par phase d'exécution (PLAN §2).
Format inspiré de [Keep a Changelog](https://keepachangelog.com/fr/) ; le
projet est en pré-alpha, sans release publiée (les jalons A1/B1/1.0 sont
définis en SPEC D-12.2).

## Phase 7 — Durcissement → A1 (en cours, 2026-06-14)

### Ajouté

- **Révocation d'appareil** (espace aidant, 7.2/SPEC §7.C) : `DELETE
  /api/v1/devices/{id}` (scope `care`, 204, idempotent) + `fluencectl revoke
  <id>`. Un appareil perdu ou compromis est coupé immédiatement (son token
  cesse d'authentifier). Couvert par 2 tests hub.

### Passe d'audit adversarial (7.7)

#### Corrigé

- **Sécurité** : la comparaison du code d'appairage est désormais **à temps
  constant** (plus de canal auxiliaire temporel révélant combien de chiffres
  correspondent ; défense en profondeur au-dessus du verrou à 5 tentatives).
- **Fiabilité (urgence)** : le composeur ne **désarme l'urgence qu'après**
  confirmation du hub ; en cas d'échec, il reste armé pour réessayer et signale
  l'erreur — il ne prétend jamais l'avoir envoyée.
- **Fiabilité** : `Composer.close()` annule tous les timers (dont le timer de
  reconnexion désormais mémorisé), interrompt la suggestion en cours et ferme la
  socket — plus de boucle de reconnexion orpheline.
- **Robustesse** : le SDK ignore une frame SSE mal formée au lieu de lever et de
  tuer le générateur de suggestions.
- **Robustesse** : la calibration ridge rejette les features/cibles non finies
  (NaN/Inf) en amont — une frame capteur corrompue ne peut pas empoisonner le
  mapping.
- **Honnêteté (PLAN §0.8)** : `measure.py` **refuse** de mesurer sans split TEST
  gelé (remplace un repli silencieux train=test qui gonflait la baseline n-gram).

#### Méthode

- Revue adversariale en 4 domaines (moteur regard, sécurité/fiabilité hub,
  client/SDK/e2e, ML/contrats/CI). Faux positifs écartés avec justification : le
  broadcast est un ring borné (256), `TargetMap` est borné par la limite de corps
  (512 KiB), la fuite P0 du draft est déjà couverte par un kill-test.

## Phase 6 — Le regard (moteur + gate d'exactitude — en cours, 2026-06-14)

### Ajouté

- **Moteur de regard complet** (`fluence-input`, SPEC §4.C/§4.D), pur et
  déterministe : filtre **One Euro** (`one_euro`), **I-VT** fixation/saccade +
  perte (`fixation`), **fusion** pondérée par confiance + « regard désigne, tête
  affine » + **magnétisme plafonné 40 %** + **modèle de bruit** par utilisateur
  (`fusion`), **calibration ridge** par profil — collecte pursuit, fit, mise à
  jour continue lissée, détection de dérive, profil versionné serde
  (`calibration`), et le **`GazePipeline`** assemblé (`gaze`).
- **Gate d'exactitude T4** : `evaluate` rejoue une session (calibration → test)
  à travers le vrai pipeline et score le % de cibles correctes (`replay`) ;
  `cargo xtask gaze-accuracy` rejoue des sessions **synthétiques** et **publie la
  précision en nightly** (`nightly.yml`), avec gate de non-régression à 0,95.
- **Câblage hub/contrat du regard** : `InputClientMessage` gagne `cal.sample`
  (paire regard brut → cible) et `cal.fit` (variantes struct, tag `k` conservé) ;
  le `/ws` fait tourner un `GazePipeline` par connexion et **route les pointeurs
  par source** — `gaze:…` à travers calibration+fusion+dwell, `mouse:…`
  directement au moteur de dwell (chemin souris **inchangé**, e2e vert). Tests
  hub déterministes (regard calibré → commit ; non calibré → rien).
- **Client regard** (`apps/web-client`, opt-in) : estimation MediaPipe (pure,
  testée) → regard brut, source webcam (MediaPipe **dynamiquement importé**,
  chargé en chunk lazy, assets **offline** locaux), calibration express (fixations
  de touches → `cal.fit`), et **`record-gaze`** (6.4) → dataset `GazeSession`
  réel compatible avec le replay Rust. Le chemin souris reste intact (e2e vert).

### Vérifié

- 47 tests `fluence-input` + tests hub regard (calibré/non-calibré) + **20 tests
  web-client** (estimation + frames regard inclus) ; chemin souris préservé
  (tests dwell/targets.patch + **suite T5 e2e verte localement**) ; contrats
  régénérés (goldens/openapi/api.d.ts), spectral 0 erreur. `xtask gaze-accuracy`
  100 % synthétique.

### Reste (pour `phase-6-done`)

- Provisionner les assets MediaPipe offline (`docs/demos/phase6-gaze.md`) +
  **session webcam réelle** (humain + caméra) = action physique. Honnêteté : les
  datasets nightly sont **synthétiques** (gate de correction), pas une
  revendication de précision réelle ; `record-gaze` produit de la **vraie**
  donnée à mesurer. ML-regard (6.5) = post-A1 par conception.

## Phase 5 — La boucle complète (terminée — 2026-06-14, `phase-5-done`)

### Ajouté

- **Alerte d'urgence de bout en bout** (D-7.4, SPEC §7.A) : `SystemEvent::Emergency
  { active, at }` diffusé sur le topic `system` à tous les clients appairés
  (bannière + sonnerie) + `POST /api/v1/system/emergency` (control, 204) +
  **runtime hub** (diffusion sur le bus *avant* le journal best-effort, D-2.6) +
  **SDK `client.emergency()`**. Câblé dans la source de vérité : goldens,
  `openapi.json`, `api.d.ts`. La double confirmation est l'affaire du composeur.
- **`fluence-input` câblé au hub** (5.1, SPEC §4.A, D-4.1) : `PUT /input/targets`
  (déclaration de la carte de cibles) ; sur le topic `input` du `/ws`, un
  `SelectionEngine` par connexion hit-teste les échantillons pointeur et fait
  tourner le dwell, estampillant les `SelectionUpdate` en `SelectionEvent`
  (focus/dwell/commit/cancel) diffusés sur le bus. Souris = source v0.
- **`worker-tts` — voix Piper + fallback OS** (5.2) : crate `fluence-voice`
  (`PiperBackend` sous-processus PCM→WAV ; `SystemVoiceBackend` SAPI/espeak-ng,
  « une voix, toujours » SPEC §2.C ; `FallbackVoice`). `POST /voice/speak`
  streame du **WAV** (ADR-0009 ; opus différé Phase 7), `GET /voice/voices`. Le
  texte P0 ne touche jamais un log. `enable_thinking=false` ajouté au backend
  LLM (Gemma E4B raisonne sinon).
- **Composeur web** (`apps/web-client`, 5.3/5.4, AGPL) : clavier au **dwell**
  (souris) + clic, 3 emplacements de suggestions fixes, **PARLER** invariant,
  **urgence** à double confirmation (bannière + sonnerie), SSE pour les
  suggestions, WS pour la sélection, anti-scintillement (1 maj/600 ms, jamais
  pendant dwell > 40 %), reconnexion, autosave draft, **i18n** (clés, FR seul)
  et thème **contraste élevé**. Servi par le hub en PWA même origine
  (`ServeDir`, `FLUENCE_WEB_DIR`).
- **Instrumentation locale** (5.5) : WPM effectif et économie de frappe réels,
  calculés côté client et affichés (« mon premier WPM réel », SPEC §1.2).
- **Suite e2e T5 « personas »** (`apps/e2e`, Playwright, AGPL — #43) : le vrai
  composeur contre le **hub assemblé** qui le sert, hermétique (n-gram fallback +
  voix OS, sans modèle lourd). Harnais qui lance le binaire hub, l'appaire par le
  vrai flux, et le crashe/relance. 4 scénarios (dwell+PARLER+autosave, suggestion
  acceptée, urgence reçue par un 2ᵉ client, hub tué→reconnexion+draft intact),
  **verts Win+Linux** (`integration.yml` job `e2e`). Démo reproductible filmée
  (`pnpm --filter @fluence/e2e demo`, Piper FR) + `docs/demos/phase5-loop.md`.

### Corrigé

- **`voice` — voix OS Windows silencieuse** (#43) : `POST /voice/speak` renvoyait
  `200 audio/wav` de **0 octet** (SAPI n'écrit rien quand le chemin est tenu
  ouvert par un `NamedTempFile`) → « une voix, toujours » violée. Répertoire temp
  + chemin interne ; test RIFF non vide.
- **`web-client` — dwell non semé** (#43) : course `PUT /input/targets` vs
  ouverture `/ws` pouvait laisser le moteur de sélection **vide** (dwell mort).
  Le composeur sème le moteur vivant via un `targets.patch` sur la socket à
  l'ouverture ; helper pur + test de régression hub `ws`.

### Vérifié

- Rust : clippy + fmt + tests (input wiring, voice handlers + 14 tests
  `fluence-voice` + smoke live Piper, urgence, **seeding `ws` par `targets.patch`**),
  `check-contracts` + spectral 0, `doc -D warnings`.
- TS : SDK 18 tests ; web-client typecheck + eslint strict + 11 tests vitest +
  build Vite. **Suite e2e Playwright verte Win+Linux** (#43). `cargo-deny`.

### Intégré

- Pile #32–#40 fusionnée sur `main` via **#41** (`c61ab06`) après résolution des
  conflits hub sur une branche d'intégration unique (contrats/`api.d.ts`
  régénérés, jamais fusionnés à la main). Tag **`phase-4-done`** posé.
- Suite T5 + fixes de fiabilité via **#43** (`53838a1`). Tag **`phase-5-done`**.

### Reste (dette)

- Rendre le job `e2e` un **check requis** de la protection de `main` (#43 a
  fusionné pendant qu'il tournait — réussi mais non bloquant). P0-scheduler
  D-3.3, opus + streaming chunké (Phase 7), stockage chiffré des métriques (P2),
  fix de génération TS de `InputClientMessage` (tag `k` perdu sur variantes
  newtype).

## Phase 4 — Le moteur : LLM réel (2026-06-13)

### Ajouté

- `LlamaServerBackend` (`fluence-inference`, ureq) : backend LLM local **réalisé
  en client HTTP pur Rust** vers un sous-processus `llama-server` (ADR-0007
  amendé — le FFI `llama-cpp-2` ne build pas sur windows-gnu, rétrogradé en
  optimisation future, #25). `generate` streaming SSE **annulable** par slot ;
  `next_chars` via `/completion` `n_predict:1`+`n_probs` → agrégation des
  log-probabilités par **premier caractère**, renormalisée ; `is_healthy()`.
  Aucune compilation C++/CMake dans le build ; crash isolé par la frontière de
  processus (D-2.6).
- **Gestion de modèles v0** (`cargo xtask download-test-assets`) : manifeste
  `models/test-assets.json` (URL + **sha256** + taille = contrat d'intégrité),
  téléchargement **repris** (`Range`), **idempotent**, `--check` sans réseau,
  cache `.fluence-cache/models` (`FLUENCE_MODELS_DIR`). Tiny-LLM SmolLM2-135M
  (mécanique uniquement, jamais la qualité — PLAN §1). Le binaire `llama-server`
  est de l'infrastructure (provisionnée CI/dev), pas un modèle géré.
- `fluence-hub` : **spawn et supervision de `llama-server`** — config
  `FLUENCE_LLAMA_SERVER_BIN`/`_MODEL`/`_CONTEXT` ; `SupervisedLlama` (moteur gated
  par un flag `ready` partagé → tant que non prêt, **dégradation n-gram
  automatique**, jamais d'attente sur une socket morte) ; supervision (port
  loopback stable, poll `GET /health`, restart à backoff, `system.degraded`,
  worker dans `/system/health`). `stdout`/`stderr` du serveur **jetés** (logs
  potentiellement P0, §9.A).
- `fluencectl suggest --mode rephrase|continue` : client SSE de l'engine,
  affiche les suggestions et leur origine (`model`/`n-gram`/`memory`).

### Vérifié

- **Kill-test LLM (critère « Done quand » #3)** : un binaire de test
  `fake-llama-server` permet à un test d'intégration de piloter le **vrai**
  chemin de supervision via le binaire hub — `/suggest` streame des suggestions
  d'origine `model` à chaud, puis **dégrade en 200 (jamais 5xx)** à la mort du
  serveur (« le clavier parle toujours », D-2.6) — hermétique, sans modèle
  lourd.
- **Pipeline réel validé localement de bout en bout** : `fluencectl suggest` →
  hub → **vrai** `llama-server` supervisé + SmolLM2 → suggestion `[model]`.

### Critère valeur #31 — atteint (ADR-0008)

- **Éval rephrase phrase-niveau + acceptation sémantique** (`fluence_eval`) :
  `rephrase` (sources/acceptors abstraits, `evaluate_rephrase`), `live`
  (cosinus, `EmbeddingAcceptor` bge-m3 via `/v1/embeddings`, `HubRephraseSource`
  pilotant le **vrai** `/suggest` du hub), `measure` **split-aware** (n-gram
  entraîné sur train+dev, tous modes évalués sur le split **test**
  hors-domaine).
- **Corpus teacher v1** (`fluence_data.teacher` + `generate`) : 12 situations ×
  4 registres sous consigne anti-pathos, parsing de transcripts, dedup par
  embedding, splits gelés (un dialogue de test par situation). Corpus **v1 :
  136 dialogues** (Gemma 4 E4B), datasheet + tests d'invariants. Misérabilisme
  filtré (écran auto 0 marqueur + **revue humaine ≥10 % approuvée**, SPEC §5.D).
- `enable_thinking=false` sur le chemin `generate` du backend : Gemma 4 E4B est
  un modèle à raisonnement qui, sinon, épuise le budget en réflexion cachée et
  ne renvoie rien — désormais il répond directement (no-op pour les autres).

### Critère amendé (ADR-0008)

- Le gate « +10 pts KS% in-domain » était un proxy biaisé (n-gram sur-appris
  35,49 % + plafond de longueur du fragment télégraphique). **Amendé** : rephrase
  doit battre le n-gram sur le **WPM** (primaire — étoile polaire ×3, SPEC §1.2)
  **et** sur le **KS% hors-domaine**. Correction méthodologique documentée, pas
  un ajustement de seuil (PLAN §0.5/§0.8).

### Vérifié (mesure locale)

- Sur le split **test** gelé de v1 (n-gram entraîné sur train+dev ; rephrase via
  le vrai hub + Gemma 4 E4B ; acceptation sémantique bge-m3) :
  lettre-à-lettre 0 % / **n-gram hors-domaine 11,71 % KS, 12,31 WPM** / rephrase
  **29,52 % KS, 20,91 WPM, acceptation 0,95** → **gate PASS** (WPM +8,60,
  KS% +17,81). Tag `phase-4-done` posé au merge ; gate CI nightly différé au
  runner self-hosted (Phase 7, ADR-0008).

## Phase 3 — La boussole : harnais d'évaluation (2026-06-13)

### Ajouté

- `fluence_data` (Python) : format de corpus versionné (pydantic) — dialogues,
  tours, variantes d'entrée sur la matrice 12 situations × 4 registres, splits
  gelés, I/O JSONL ; matrice de **confusion spatiale AZERTY** ; générateurs de
  variantes (télégraphique, bruitée, abrégée) ; grille **anti-pathos** (juge
  auto) ; **corpus v0** (graine de 15 dialogues écrite à la main, anti-pathos,
  golden `corpus/v0.jsonl`).
- `fluence_eval` (Python) : **métriques** KS%, WPM simulé, taux d'acceptation et
  de suggestions nuisibles — comptabilité en entiers, déterministe au bit près
  (Win/Linux) ; **utilisateur simulé** (dwell + fatigue, coût de scan facturé
  350 + 150 ms, acceptation lexicale v0) ; **sources** lettre-à-lettre, oracle,
  n-gram ; runner → `EvalReport` versionné ; CLI `run`/`check` et
  `python -m fluence_eval`.
- `fluence-ngram` (Rust) : modèle fréquentiel FR compact — `complete` (mots) et
  `next_char_dist` (distribution caractère), entraînable, sérialisable JSON —
  le **fallback D-2.6** réutilisable par le hub ; binaire `serve` (protocole
  JSON-lines) que l'éval pilote pour mesurer le vrai modèle (ADR-0006 amendé).
- `cargo xtask run-eval [--suite]` opérationnel (n'« exit 2 » plus) ; **porte de
  CI** : régression KS% > 2 points = échec (test de baseline gelée) ; delta KS%
  par mode publié au résumé de job CI.
- ADR-0006 (architecture du harnais ; staging v0→v1 vs §8.A).

### Vérifié (l'encadrement auto-valide le harnais — SPEC §8.A)

- lettre-à-lettre = 0 % KS (plancher) < n-gram réel = 35,5 % KS < oracle =
  66,8 % KS (plafond), sur le corpus v0 ; déterminisme seedé ; propriétés de la
  matrice de confusion (Σ ≈ 1, voisin proche > lointain). Différés en dette :
  corpus v1 par teacher LLM (#18), commentaire PR du delta (#19).

## Phase 2 — Hub & supervision : « le clavier parle toujours » (2026-06-13)

### Ajouté

- `fluence-ipc` : couche IPC hub↔workers — frames JSON préfixées longueur
  (cap 16 MiB) sur UDS (Linux) / named pipes (Windows) derrière une API
  unique ; protocole v0 (Hello/Ping/Echo/Shutdown). ADR-0005.
- `fluence-store` : persistance chiffrée SQLCipher (AES-256) ; clé en
  keystore OS (ou fichier en test) ; migrations versionnées ;
  acteur mono-thread à file de commandes (ordre d'écriture garanti) ;
  WAL + `synchronous=FULL` (durabilité ≤ 1 s, D-2.6) ; tokens stockés
  hashés (SHA-256) ; journal d'accès sans donnée P0.
- `fluence-hub` : bootstrap (< 3 s, config TOML+env, port 7411 + repli
  dynamique), tracing à **redaction P0** (types `SecretString` + denylist
  de champs), arrêt propre ; appairage (fenêtre 2 min, code à usage unique,
  verrouillage anti-brute-force → 429), tokens à scopes
  (display/control/care/system), middleware d'auth (401 uniforme), CORS
  allowlist stricte ; superviseur (backoff exponentiel + jitter, états,
  événements `system.degraded`) ; WebSocket `/ws` multiplexé par topics
  filtrés par scope ; autosave du draft ; `/system/health` +
  `/system/capabilities` ; worker `worker-echo` pour les kill-tests.
- Routes ajoutées au contrat via la chaîne anti-dérive : `POST /pair/window`
  (system), `GET /sessions/{id}/draft` (reprise de session §2.A).

### Vérifié (kill-tests, le cœur de la phase)

- worker tué → `system.degraded` < 500 ms (provisional) + relance à backoff
  + compteur de restarts exposé ; hub tué (-9) en pleine frappe → draft
  restauré, perte ≤ 1 s ; 50 cycles kill/restart → RSS stable (±10 %) ;
  démarrage → prêt < 3 s ; aucun contenu P0 dans les logs (bout en bout).

### Durci (audit adverse — §2.C « le clavier parle toujours », §9.A « zéro P0 »)

- **Durabilité du draft (F01)** : le flush ne vide plus le buffer *avant*
  l'écriture — il snapshote, écrit par lot, puis ne retire que ce que le
  store a confirmé (et seulement si aucune frappe plus récente n'est
  arrivée). Sur erreur du store, tout reste bufferisé pour le tick suivant :
  une frappe acquittée ne peut plus disparaître à la fois de la RAM et du
  disque (D-2.6).
- **Bornes de ressources contre un appareil appairé hostile (F09, F15, G7)** :
  cap du texte de draft (64 KiB, rejeté *avant* de devenir un `SecretString`,
  sans P0 dans le 422) ; cap des sessions non flushées en RAM (le dépassement
  force un flush immédiat, jamais une perte ni un blocage de frappe) ; purge
  disque des drafts inactifs > 7 jours via un index de récence (migration
  store v2) ; plafonds de connexions `/ws` par appareil (8) et globaux (128),
  réservés avant l'upgrade et libérés par une garde RAII sur tout chemin de
  sortie ; plafond explicite du corps de requête (512 KiB).
- **Dégradation honnête au démarrage (F06, F07, G2, F30)** : avertissement à
  chaque boot si la clé du store est un fichier en clair (escaladé s'il
  jouxte `store.db`) ; le fichier system-token est écrit *avant* la ligne
  d'appareil (pas d'appareil `system` orphelin re-créé à chaque tentative) ;
  un échec d'étape d'installation (dossier de données, token, `chmod 0600`)
  remonte en `HubError::Setup` au lieu d'être avalé ou mal étiqueté.

## Phase 1 — Le contrat (2026-06-13)

### Ajouté

- `fluence-protocol` complet : 100 % des messages et endpoints des SPEC
  §4.A (FluenceInput v1), §5.A (API du hub), §5.B (mémoire), §2.A
  (appairage, scopes, WebSocket) — invariants dans les types (`Normalized`
  rejette les hors-bornes à la désérialisation), enveloppe d'erreur
  RFC 9457 avec catalogue de codes stables.
- Registre de routes déclaratif (`routes()`) : la surface API comme données
  testées (unicité, préfixe, scopes, stabilité `stable`/`experimental`).
- Chaîne de contrats `cargo xtask check-contracts` : 28 goldens JSON Schema
  + OpenAPI 3.1 + types TS générés (`openapi-typescript`), dérive = erreur
  de CI (job `contracts (T3)`, lint spectral à zéro), prouvée par mutation.
- `@fluence/sdk` v0 : client typé (fetch + SSE + WebSocket), zéro logique
  métier, erreurs problem+json typées, parser SSE robuste à la
  fragmentation, 17 tests dont tests de types.
- Doc API publiée sur GitHub Pages (Redoc + rustdoc) ; gate de couverture
  `fluence-protocol` ≥ 85 % en CI.
- ADR-0004 (décisions de contrat v1).

## Phase 0 — L'usine (2026-06-13)

### Ajouté

- Monorepo §2.B : workspaces Cargo (7 crates + xtask), pnpm (3 packages),
  uv (3 packages ml) — chaque unité testée, qualité bloquante (clippy
  pedantic, `forbid(unsafe_code)`, TS strict, mypy strict).
- Licences par couche (D-10.1) : briques Apache-2.0, applications
  AGPL-3.0-only ; en-têtes SPDX vérifiés par `cargo xtask check-licenses`
  (le dépôt se vérifie lui-même) ; `cargo-deny` bloquant.
- 4 workflows CI (ci, integration, nightly, release dry-run) en matrice
  Windows + Linux ; protections de branche (squash-only, checks requis) ;
  hooks lefthook < 5 s ; conventional commits vérifiés (hook + CI).
- Gouvernance : CONTRIBUTING, SECURITY (divulgation coordonnée, D-9.3),
  CODE_OF_CONDUCT, templates PR/issues (dont `debt`), template ADR.
- ADR-0001 (hub/workers), ADR-0002 (monorepo & contrats), ADR-0003
  (outillage Phase 0).
