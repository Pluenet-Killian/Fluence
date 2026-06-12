# SPEC — Fluence

> **Plateforme de communication open source, locale et intégrée, pour le handicap moteur lourd.**
> Document de spécification produit & technique. Co-défini par décisions successives.
>
> **Statuts** : ✅ décidé · 🔶 proposition en attente de décision · ⬜ ouvert (pas encore instruit)
> **Méthode** : les décisions sont numérotées `D-<section>.<n>` et journalisées en Annexe A. Rien ne se code tant que la section concernée n'est pas ✅.
> **Version** : 0.4 — 2026-06-13 · **Sources** : `Project.md` (rapport de recherche), sessions de cadrage.
> **État : spec complète.** Toutes les décisions de conception sont ✅ (specs détaillées en sous-sections .A/.B/.C/.D). Restent ⬜ deux items qui sont des *actions/benchmarks de P1*, pas des specs : D-3.4 (choix ASR — tranché par benchmark) et D-10.3 (contact upstream AsTeRICS Grid).
> **Vocabulaire** : **P1/P2/P3** = phases *produit* (§12, en mois). **Phase 0–7** = étapes *d'exécution* du `PLAN.md` (à l'intérieur de P1). **A1/B1/1.0** = jalons (D-12.2). **D-x.y** = décisions. Ne jamais confondre « P3 » (an 2) et « Phase 3 » du PLAN (harnais d'éval).

---

## 1. Vision & cadrage

### 1.1 Mission ✅
Ramener une personne en situation de handicap moteur lourd (SLA en tête) de 8–10 mots/minute à **30+ mots/minute effectifs en conversation réelle**, et l'accompagner de l'annonce du diagnostic jusqu'aux stades les plus avancés — en français d'abord, hors-ligne, avec des données qui ne quittent jamais le foyer.

### 1.2 Étoile polaire & thèse ✅
- **Métrique unique de succès : le débit conversationnel effectif (×3), mesuré en usage réel consenti.** Toute feature se justifie par sa contribution à cette métrique (directe : vitesse ; ou indirecte : fatigue, abandon, temps de configuration).
- **Thèse** : personne n'occupe l'intersection des trois fronts — (a) intégration verticale entrée + IA + voix, (b) IA locale de qualité (seuil franchi par les modèles ~4B en 2025-2026), (c) open source local-first. Tobii/Smartbox ont (a) sans (b, local) ni (c) ; les libres ont (c) sans (a) ni (b) ; la recherche a prouvé (b) sans industrialiser. Analogie de positionnement : **Home Assistant** — le produit intégré local-first qui devient à la fois la référence ET l'écosystème.
- **Différenciateur défendable** : *le système apprend la personne (style, voix, proches, routines) et tout reste chez elle.*

### 1.3 Utilisateur primaire & personas ✅
- **Primaire** : adulte lettré avec atteinte motrice lourde évolutive ou stable (SLA, locked-in, myopathies avancées, tétraplégie haute) — **texte libre d'abord**, pas grilles pictographiques.
- **Secondaires** : l'aidant familial (installe, dépanne, configure) ; l'ergothérapeute/orthophoniste (paramètre, suit la progression).
- **D-1.1 ✅ Personas de référence** *(2026-06-13 — toute discussion produit se fait en les nommant ; à confronter au terrain dès l'alpha)* :
  - **Claire, 58 ans** — SLA diagnostiquée il y a 18 mois, parole perdue, motricité des mains résiduelle déclinante. Tracker IR d'occasion + PC portable familial (16 Go). Mariée à **Jean (63 ans, aidant principal, peu technophile)**, deux enfants adultes, trois petits-enfants. Veut : suivre le rythme des conversations familiales, garder son humour pince-sans-rire, téléphoner à sa sœur. Redoute : devenir « celle qu'on attend ». — *Persona des fonctions conversationnelles et de la voix personnelle.*
  - **Marc, 34 ans** — locked-in partiel post-AVC du tronc, regard seul, budget serré : **webcam uniquement**, vit en MAS (établissement), PC 8 Go (palier réduit). Veut : appeler l'aide-soignante, sa musique, écrire à ses amis sur Matrix, qu'on ne touche pas à sa playlist. — *Persona du webcam-only, du palier réduit et des raccordements.*
  - **Sophie, 45 ans** — ergothérapeute en centre SLA, équipe 15 patients/an, 30 min par installation maximum, doit télé-assister des familles à 80 km. — *Persona de l'espace aidant, de l'onboarding et de la matrice de compatibilité.*

### 1.4 Langues ✅
Architecture language-agnostic ; **le français est la langue d'évaluation et de référence** (corpus, benchmark, voix). EN/DE/ES/IT suivent quand le harnais d'éval les couvre.

### 1.5 Non-objectifs ✅
- Pas de BCI invasif (mais protocole d'entrée prêt à accueillir un standard type Apple BCI/Switch Control).
- Pas de hardware propriétaire (webcams, trackers du commerce, PC standards).
- Pas de cloud obligatoire, jamais. Le cloud est un boost opt-in.
- Pas de CAA pédiatrique/cognitive en v1 (servie via l'interopérabilité, cible possible en v2).

---

## 2. Architecture système

### Acquis ✅
- Cascade de backends d'inférence du plus local au plus distant, **un seul protocole** (OpenAI-compatible pour le LLM).
- Chaque couche est publiée aussi comme brique interopérable (voir §10).

### Décisions
- **D-2.1 ✅ Forme de déploiement : hybride app + hub détachable** *(2026-06-12)*. App desktop double-clic (UI + hub embarqué) ; le hub peut tourner seul (headless) et servir des clients web sur le réseau local (tablette au lit, écran interlocuteur). Conséquence d'architecture : **l'UI ne parle au hub que via l'API réseau locale, même embarquée** (un seul chemin de code, le mode déporté est gratuit).
- **D-2.2 ✅ Plateformes natives 1.0 : Windows + Linux** *(2026-06-12)*. macOS en 1.x selon demande. Clients d'affichage web : tout navigateur moderne (couvre tablettes Android/iPad via le hub).
- **D-2.3 ✅ Stack : Rust + TypeScript + Python** *(2026-06-12)*. Rust = hub (runtime IA, input engine, fusion, scheduling, API). TypeScript = UI (Tauri webview + clients web) et SDK public. Python = pipelines ML offline (données, LoRA, éval). Bindings natifs : llama.cpp / whisper.cpp / ONNX Runtime via FFI Rust.
- **D-2.4 ✅ Topologie & sécurité réseau** *(2026-06-13, spec en §2.A)*.
- **D-2.5 ✅ Monorepo** *(2026-06-13, spec en §2.B)*.
- **D-2.6 ✅ Modèle de fiabilité « le clavier parle toujours »** *(2026-06-13, spec en §2.C)*.

### 2.A Spec — Topologie & sécurité réseau (D-2.4)

**Deux modes, un seul code :**
- **Mode embarqué (défaut)** : le hub écoute sur `127.0.0.1:7411` (configurable, repli dynamique si occupé). Pas de TLS sur loopback. L'UI Tauri s'y connecte comme n'importe quel client.
- **Mode foyer (opt-in)** : écoute LAN + annonce **mDNS** `_fluence._tcp.local` (TXT : version API, nom du foyer, empreinte de la CA locale). **HTTPS obligatoire hors loopback.**

**PKI locale, zéro dépendance externe (offline-first)** : à la première exécution, le hub génère une CA locale + un certificat feuille. Les clients installés (Tauri) épinglent la CA à l'appairage (vrai pinning). Les clients navigateur purs passent par un avertissement TLS accepté une fois (TOFU) — limite assumée et documentée ; l'écran d'appairage affiche l'empreinte à comparer.

**Appairage** : code à 8 chiffres (ou QR) affiché sur l'écran principal → `POST /pair {code, device_name, device_kind}` → le client reçoit `{ca_cert, device_token}`. Tokens **par appareil, révocables** depuis l'espace aidant, à **scopes** :
| Scope | Donne accès à | Exemple d'appareil |
|---|---|---|
| `display` | texte à afficher, état de parole — lecture seule | écran face à l'interlocuteur |
| `control` | composeur complet, suggestions, TTS, entrée | tablette/PC de l'utilisateur |
| `care` | configuration, mémoire (selon permissions), diagnostics | téléphone de l'aidant |
| `system` | tout (réservé UI embarquée et CLI locale) | app desktop |

**Défense du port local** (attaque « drive-by localhost » depuis un site web visité) : token obligatoire sur **toutes** les routes (header `X-Fluence-Token`), CORS en allowlist stricte (origines appairées uniquement), aucun cookie. Seules routes accessibles sans token : `GET /pair/info` et `POST /pair` — et **l'appairage n'est possible que pendant une fenêtre ouverte explicitement depuis l'UI principale** (2 min, rate-limité, code à usage unique) *(précisé en review : sinon l'appairage serait impossible… ou brute-forçable)*. Journal d'accès local consultable dans l'espace aidant.

**Canal temps réel** : WebSocket `/ws` unique, multiplexé par topics (`input`, `asr`, `suggest`, `voice`, `system`), abonnement selon scope. Heartbeat 5 s ; reconnexion automatique avec reprise de session (les drafts et sessions survivent aux coupures).

### 2.B Spec — Organisation du code (D-2.5)

**Monorepo GitHub** (refactorings transverses fréquents en phase jeune, CI unifiée avec le harnais d'éval ; les briques sont *publiées depuis* le monorepo — crates.io/npm — si des consommateurs externes apparaissent) :

```
fluence/
├─ crates/                    # Rust (Cargo workspace)
│  ├─ fluence-hub/            # binaire : API HTTP/WS, supervision, sélection
│  ├─ fluence-protocol/       # ★ SOURCE DE VÉRITÉ des types/schémas (schemars)
│  ├─ fluence-inference/      # workers : llama.cpp / ASR / TTS / embeddings (FFI)
│  ├─ fluence-input/          # drivers, fusion, calibration, dwell/scan
│  ├─ fluence-accel/          # contexte, retrieval mémoire, prompts, post-traitement
│  ├─ fluence-voice/          # pipeline voix (Piper + clonage)
│  └─ fluence-store/          # persistance chiffrée (profils, mémoire, journaux)
├─ apps/
│  ├─ desktop/                # Tauri v2 (embarque et supervise le hub)
│  ├─ web-client/             # PWA : composeur, écran interlocuteur, espace aidant
│  └─ cli/                    # fluencectl : debug, bench, replay
├─ packages/                  # TypeScript (pnpm workspace)
│  ├─ sdk/                    # @fluence/sdk — client API public (généré + ergonomie)
│  ├─ ui/                     # composants accessibilité partagés
│  └─ integrations/           # asterics-grid-plugin, dasher-lm
├─ ml/                        # Python (uv)
│  ├─ data/                   # génération synthétique, préparation corpus
│  ├─ training/               # LoRA (distillation)
│  └─ eval/                   # harnais de simulation (aussi exécuté en CI)
├─ models/                    # manifestes du registre de modèles (pas les poids)
├─ docs/                      # dont docs/adr/ (architecture decision records)
└─ .github/workflows/
```

**Contrats** : `fluence-protocol` (Rust + `schemars`) génère JSON Schema → OpenAPI 3.1 + types TS du SDK ; la CI échoue si les artefacts divergent. **CI** : lint/test/build en matrice Windows+Linux ; harnais d'éval en sous-ensemble rapide par PR, complet en nightly ; benchs de latence sur runners self-hosted (les machines de référence, cf. D-11.1). Erreurs API : `application/problem+json` (RFC 9457). Conventional commits ; toute décision d'architecture nouvelle = un ADR dans `docs/adr/`.

### 2.C Spec — Fiabilité & supervision (D-2.6)

**Principe cardinal : composer et vocaliser ne dépendent JAMAIS de la santé des composants IA.**

- **Isolation par processus** : `fluence-hub` (superviseur + API + entrée + sélection + store) lance des **workers d'inférence en processus enfants** (`worker-llm`, `worker-asr`, `worker-tts`, `worker-embed`). Un crash de lib native (GGML/ONNX) ne tue jamais l'entrée. IPC : **UDS sur Linux / named pipes sur Windows** *(corrigé en review : AF_UNIX Windows n'est pas supporté par tokio)*, abstraits derrière une couche IPC commune, messages JSON préfixés longueur (débogable ; optimisation binaire plus tard) ; audio via ring buffer en mémoire partagée. Redémarrage avec backoff exponentiel + événement `system.degraded` vers les UI.
- **Chaîne de dégradation explicite** :
| Panne | Comportement | L'utilisateur garde |
|---|---|---|
| worker-llm down | prédiction **n-gram FR embarquée dans le hub** (quelques Mo, toujours chargée) ; reformulation/réponses masquées, bandeau discret | clavier + prédiction basique + TTS |
| worker-asr down | contexte conversationnel gelé | tout le reste |
| worker-tts down | repli **voix système OS** (SAPI / espeak-ng) | une voix, toujours |
| hub down | mode embarqué : watchdog de l'app desktop, relance < 2 s · mode headless : systemd `Restart=always` (Linux) / recovery du service Windows ; le client garde le draft (autosave local continu) | son texte, l'UI se reconnecte seule |
| coupure courant | write-ahead + autosave draft (débounce 500 ms) | ≤ 1 s de frappe perdue, garanti |
- **Démarrage progressif** : hub opérationnel (entrée + clavier + TTS de repli) **< 3 s** ; modèles IA en chargement arrière-plan priorisé (TTS → LLM → ASR → embeddings), l'UI affiche l'état sans bloquer.

---

## 3. Runtime IA

### Acquis ✅
- **Moteurs** : llama.cpp (LLM, Vulkan/CUDA/CPU), ASR (choix D-3.4 : Voxtral Realtime / whisper.cpp / audio natif Gemma 4), Piper (TTS temps réel), moteur de clonage (§6.A), ONNX Runtime (regard, embeddings).
- **Flotte par palier matériel** *(corrigé en review 2026-06-13 : l'OS et l'app consomment 3–4 Go — la flotte doit tenir dans le reste)* :
  - **Palier réduit (8 Go RAM, sans GPU — FLU-REF-1)** : ~3,5 Go — LLM E2B Q4 + LoRA · ASR compact (chargé seulement si écoute active) · embeddings à la demande · Piper FR · n-gram de secours. Les budgets de latence (§5.A) sont définis sur CE palier.
  - **Palier nominal (16 Go)** : ~8 Go — E4B Q4 + LoRA · ASR complet résident · embeddings résidents · Piper + voix personnelle.
  - **Palier hub GPU (≥ 12 Go VRAM)** : 26B A4B (MoE) · ASR complet · F5-TTS temps quasi réel.
  - L'affectation est automatique au profilage d'installation (D-3.2) et consultable/modifiable dans l'espace aidant.
- **Orchestration** : runtime unifié avec ordonnancement par priorités (TTS prioritaire absolu, ASR en continu, LLM en rafales) ; profilage matériel à l'installation (choix automatique tailles/quantifications) ; dégradation gracieuse jusqu'à un mode ≤ 4 Go.
- **Modèle par défaut** : hypothèse de travail **Gemma 4 E4B** (sortie 2026-04-02, **Apache 2.0**, ~4,5B effectifs, conçu edge/laptop, multimodal avec audio en entrée). Cascade selon matériel : **E2B** (~2,3B) pour le mode ≤ 4 Go · **E4B** par défaut · **26B A4B (MoE, 3,8B actifs)** pour hub avec GPU consumer — qualité supérieure à coût d'inférence proche du 4B. **Le benchmark interne tranche sur chiffres** (qualité tâche FR × latence × empreinte) ; panel de comparaison : Qwen3, Ministral, autres. Piste à évaluer (D-3.4) : l'entrée audio native d'E4B comme alternative à whisper.cpp pour le contexte conversationnel.

### Décisions
- **D-3.1 ✅ Politique cloud : local par défaut + opt-in granulaire** *(2026-06-13)*. Tout fonctionne 100 % local. Activation fonction par fonction d'un endpoint distant (clé API perso ou serveur familial), visible, réversible, jamais silencieuse. Le cloud est un backend de plus derrière le même protocole OpenAI-compatible.
- **D-3.2 ✅ Gestion des modèles** *(2026-06-13)*. Registre = manifestes versionnés dans `models/` : `{id, version, rôle, fichiers (sha256, taille), sources (HF + miroir), licence, palier matériel min}` — **signés (minisign)**, vérifiés au téléchargement (reprise sur coupure). **Principe de stabilité comportementale : un modèle ne change JAMAIS silencieusement** — toute mise à jour de modèle est proposée, expliquée (« la voix/les suggestions peuvent changer »), refusable, réversible. GC : modèles inutilisés > 90 j → proposition de suppression. **Pack hors-ligne** : installeur USB complet (app + modèles du palier) pour les foyers/établissements sans bon réseau — réalité du terrain.
- **D-3.3 ✅ Scheduler du runtime** *(2026-06-13)*. Files par priorité stricte : **P0 TTS** (interactif, préempte tout) > **P1 `next-chars`/`suggest`** (annulables par slot) > **P2 ASR** steady-state > **P3 fond** (embeddings, résumés §5.C, extraction mémoire §5.B, entraînements voix). Une requête P0 préempte une génération P1 en cours (elles sont annulables par construction). Budgets mémoire par worker négociés au boot selon le palier (§3) ; pas de swap silencieux : si la pression mémoire monte, le hub décharge explicitement (événement `system.degraded`, le moins prioritaire d'abord). Latences p50/p95 par classe exposées dans `/system/health` et tracées en continu.
- **D-3.4 ⬜ Choix du moteur ASR** — benchmark P1 à **trois candidats** : (a) **Voxtral Realtime** (Mistral) — encodeur audio causal, vrai streaming ~480 ms de délai, WER 5,9 % vs 7,4 % pour Whisper sur FLEURS, 13 langues dont FR (licence des variantes 2026 à vérifier) ; (b) **whisper.cpp** (large-v3-turbo quantifié) — le choix éprouvé, outillage mature ; (c) **audio natif Gemma 4 E4B** — un seul modèle pour contexte + génération. Critères : WER FR conversationnel, latence streaming, diarisation, empreinte mémoire en cohabitation avec le LLM.

---

## 4. Input Engine

### Acquis ✅
- **Abstraction fondatrice** : tout capteur produit un flux normalisé (position, confiance, événements de sélection) ; le reste de la plateforme est agnostique de la modalité. Protocole publié (WebSocket/UDP) — compatible écosystème (Opentrack, Miranda, OptiKey).
- **Drivers 1.0** : trackers IR via émulation souris (universel) et protocoles ouverts — stratégie par niveaux D-4.2, **aucun SDK propriétaire prérequis** ; regard webcam (MediaPipe + mapping few-shot, §4.C) ; head-tracking (compatible opentrack) ; contacteurs USB/BLE ; clignement/mimiques.
- **Fusion multimodale** : regard (zone) + tête (affinage), regard + contacteur (validation) ; filtrage adaptatif, modèle de bruit par utilisateur. C'est ce qui rend le webcam-only réellement utilisable.
- **Calibration continue implicite** : recalibrage silencieux sur les sélections réussies ; détection de dérive ; recalibration express 3 points / 10 s ; profils par contexte (lit, fauteuil, lumière). Réponse directe au « human debugging » des aidants.
- **Sélection** : dwell adaptatif **piloté par le modèle de langage** (durée modulée par la probabilité linguistique de la cible), scanning multi-vitesses, switch.

### Décisions
- **D-4.1 ✅ Protocole d'entrée FluenceInput v1** *(2026-06-13, spec en §4.A)*.

### 4.A Spec — Protocole d'entrée FluenceInput v1 (D-4.1)

**Architecture en trois étages** — toute la décision vit dans le hub, les UI ne font qu'afficher :
1. **Sources** (drivers) → échantillons normalisés ;
2. **Moteur de sélection** (hub, Rust) : fusion, filtrage, hit-testing, dwell/scan — c'est ici que les priors linguistiques du moteur d'accélération modulent le dwell (boucle interne hub, sans passer par l'UI) ;
3. **Événements de sélection** → UI.

Ce placement rend la boucle regard→cible→langage testable en simulation (replay), indépendante de l'UI, et partagée par tous les clients.

**Messages (JSON sur WS topic `input` ; champ `v:1` négocié à l'ouverture) :**

```jsonc
// Étage 1 — sources → hub (fréquence native du capteur, 30–120 Hz)
{"k":"ptr","t":123456789,"src":"gaze:webcam0","x":0.41,"y":0.77,"conf":0.86,
 "pose":{"yaw":-3.1,"pitch":1.2,"roll":0.4}}          // pose si disponible
{"k":"sw","t":123456789,"src":"switch:ble0","btn":1,"state":"down"}

// L'UI déclare ses cibles (PUT /input/targets, patchs incrémentaux via WS)
{"surface":"main","viewport":{"w":1920,"h":1080},
 "targets":[{"id":"key_e","rect":[10,500,120,90],"role":"key","label":"e"},
            {"id":"sug_1","rect":[10,80,600,90],"role":"suggestion"}]}

// Étage 3 — hub → UI
{"k":"sel.focus","target":"key_e","t":...}
{"k":"sel.dwell","target":"key_e","progress":0.62,"eta_ms":140}   // jauge UI
{"k":"sel.commit","target":"key_e","method":"dwell","t":...}
{"k":"sel.cancel"}
{"k":"scan.highlight","group":"row:2"}                            // scanning
```

- Coordonnées normalisées [0..1] **par surface** ; le mapping capteur→surface (calibration) appartient au hub, par surface et par profil de contexte (lit/fauteuil).
- **Chemins de données** *(précisé en review)* : les drivers gérés par le hub lui-même (webcam locale, UDP, HID) court-circuitent le WS — in-process direct. Le topic WS `input` sert aux **capteurs des clients distants** (ex. : la tablette au lit fait tourner MediaPipe en web et publie ses échantillons au hub) et aux outils de debug/replay.
- **Interop entrante** : serveur UDP optionnel — compatible **Opentrack** (port 4242, head-pose) et format `FluenceInput-UDP` documenté pour trackers tiers. **Interop sortante** (1.x) : émulation souris OS pour piloter d'autres applications (à la TD Control).
- **Budgets** : traitement d'un échantillon < 5 ms ; commit → événement UI < 20 ms ; le dwell adaptatif lit les priors dans un cache partagé rafraîchi à chaque frappe (< 30 ms, cf. §5.A `next-chars`).
- Sécurité : publier des échantillons ou des cibles exige le scope `control`.
- **D-4.2 ✅ Stratégie trackers par niveaux** *(2026-06-13, spec en §4.B)*.
- **D-4.3 ✅ Pipeline de fusion v1** *(2026-06-13, spec en §4.C)*.
- **D-4.4 ✅ Politique de calibration** *(2026-06-13, spec en §4.D)*.

### 4.B Spec — Stratégie trackers par niveaux (D-4.2)

Le risque juridique est réel : les licences grand public Tobii (gaming) **interdisent historiquement l'usage assistif** (exclusivité Tobii Dynavox). Plutôt que de bloquer sur l'audit, une stratégie par niveaux qui fonctionne quoi qu'il arrive :
- **Niveau 0 — universel** : tout tracker configuré en **émulation souris** par son logiciel constructeur fonctionne d'office (source `mouse`). Couvre les dispositifs Tobii Dynavox existants, sans toucher à leur SDK.
- **Niveau 1 — protocoles ouverts** : **OpenGaze API** (GazePoint — trackers abordables, protocole TCP documenté), **Opentrack/UDP** (head-trackers), **FluenceInput-UDP** (notre format publié, pour tout constructeur ou bricoleur qui veut être compatible).
- **Niveau 2 — webcam native** : notre pipeline MediaPipe + modèle de regard maison (§4.C) — zéro matériel.
- **Niveau 3 — SDK propriétaires** : par driver, **après audit juridique individuel** (Irisbond, EyeTech ont des programmes développeurs à examiner ; Tobii consumer : présumé exclu pour l'AT, à documenter honnêtement). Aucun SDK propriétaire n'est un prérequis de la 1.0.
- Livrable P1 : **matrice de compatibilité publiée** dans la doc (tracker × niveau × testé/communauté).

### 4.C Spec — Pipeline de fusion v1 (D-4.3)

```
capteur → prétraitement par source → filtre 1€ → détecteur fixation/saccade ─┐
capteur → …                                                                  ├→ fusion → magnétisme → sélection
(head)  → …                                                                  ┘
```
1. **Prétraitement** (par type de source) — regard webcam : MediaPipe Face Landmarker → features géométriques (iris, pose tête, géométrie écran) → **mapping few-shot vers l'écran** → point brut + confiance. *Trajectoire : v0 = régression polynomiale/ridge par profil (simple, débogable) ; cible = modèle ONNX maison type WebEyeTrack (piste ML-regard du PLAN), qui ne remplace la régression que lorsqu'il la bat sur nos datasets.*
2. **Filtre One Euro** par source (le standard du pointage temps réel : lisse au repos, réactif en mouvement — paramètres `β`, `f_cmin` exposés par profil).
3. **Détecteur fixation/saccade** (I-VT, seuil de vitesse) : **le dwell ne progresse que pendant les fixations** ; les saccades n'annulent pas la jauge (tolérance aux micro-pertes).
4. **Fusion multi-sources** : moyenne pondérée par confiance (inverse de variance estimée) ; mode **« regard désigne, tête affine »** : le regard définit une zone d'intérêt (~3° de rayon), la pose de tête contrôle un offset fin à gain réglable dans cette zone — c'est le mode qui rend la webcam utilisable sur des cibles moyennes.
5. **Magnétisme linguistique** : attraction vers les cibles pondérée par `prior` (§4.A) — force plafonnée (jamais > 40 % de la distance inter-cibles : l'utilisateur doit toujours pouvoir atteindre une touche improbable ; l'agentivité prime).
- **Modèle de bruit par utilisateur** : variance des fixations estimée en continu sur les commits réussis → dimensionne la taille effective des cibles (l'UI adapte), le seuil du magnétisme et le rayon de fusion. C'est la boucle qui personnalise la précision.
- Sortie : `{point, confiance, état: fixation|saccade|perdu}` à fréquence d'entrée ; `perdu` > 800 ms → pause douce du dwell + indicateur.

### 4.D Spec — Politique de calibration (D-4.4)

- **Initiale (jeu, ~45 s)** : cible animée en **poursuite lisse** (smooth pursuit — plus facile que fixer des croix, donne 10× plus d'échantillons, adapté aux capacités oculomotrices variées) sur 9 zones ; entraîne le mapping few-shot (tête de régression du modèle ONNX, par profil).
- **Continue implicite** : chaque `sel.commit` **non suivi d'une correction** (heuristique : pas d'effacement < 3 s) fournit une paire (features → centre de cible) → buffer glissant pondéré confiance/récence → mise à jour douce du mapping. Jamais de saut perceptible (lissage des mises à jour).
- **Détection de dérive** : erreur médiane estimée sur les commits récents > 1,5× la taille de touche pendant > 30 s → proposition discrète de recalibration express (jamais d'interruption autoritaire).
- **Express (10 s)** : 3 points, déclenchable par l'utilisateur (cible dédiée du composeur) ou l'aidant (à distance).
- **Profils de contexte** : calibrations nommées (lit/fauteuil/...) ; bascule manuelle ou suggérée si la pose moyenne change durablement. **Qualité visible** : erreur estimée en temps réel dans l'espace aidant (fin du débogage à l'aveugle).

---

## 5. Moteur d'accélération (le cœur)

### Acquis ✅
- **Quatre fonctions, une infrastructure** :
  1. `suggestReplies(ctx)` — réponses en contexte, zéro frappe ;
  2. `rephrase(fragmentBruité, ctx, style)` — télégraphique → phrase dans le style de la personne, tolérante aux erreurs de frappe oculaire ;
  3. `expand(abbr, ctx)` — expansion d'abréviations (mode expert) ;
  4. `nextTokenDistribution(préfixe, ctx)` — alimente dwell adaptatif, scanning pondéré, mode de saisie continue type Dasher.
- **Hiérarchie d'usage** : reformulation et réponses suggérées d'abord (zéro apprentissage), abréviations en mode expert. Les gains majeurs viennent du **contexte conversationnel**.
- **Contexte** : ASR local streaming de l'interlocuteur (moteur tranché par benchmark, D-3.4) + diarisation légère + historique + situation (heure, interlocuteur déclaré — opt-in). Privacy tiers : opt-in par conversation, indicateur visible, traitement local, aucun stockage du flux audio brut.
- **Mémoire personnelle (RAG local) — complète dès la 1.0 (D-5.2)** : embeddings locaux + base vectorielle chiffrée — lexique (noms, lieux, routines), phrases récurrentes, **anecdotes datées, graphe des proches, historique de conversations indexé**, avec UX de gestion de la mémoire, droits d'accès aidants différenciés et droit à l'oubli. La conversation quotidienne est répétitive : l'exploiter est un levier majeur inexploité. *C'est un sous-système de premier rang avec sa propre spec (D-5.6).*
- **Personnalisation** : profil de style (prompt) en 1.0 → **LoRA personnel** (entraîné localement/sur le hub) en P3. Règle d'or d'agentivité : l'utilisateur peut toujours éditer/rejeter d'un geste ; le système parle *comme elle*, pas comme un assistant.
- **Ingénierie de latence** : KV-cache persistant du contexte (seul le suffixe change), 3 suggestions en une passe structurée, streaming annulable à chaque frappe, décodage contraint.

### Décisions
- **D-5.1 ✅ Stratégie modèle 1.0 : prompts + LoRA de tâche distillés** *(2026-06-13)*. P1 démarre en prompt-engineering sur le modèle retenu par benchmark (hypothèse **Gemma 4 E4B**, Apache 2.0) ; dès que le harnais d'éval tourne : LoRA par tâche (reformulation, réponses, abréviations) entraînés sur données distillées d'un grand modèle. Objectifs : qualité FR, latence (prompts courts), robustesse au bruit de frappe oculaire.
- **D-5.2 ✅ Mémoire personnelle complète dès la 1.0** *(2026-06-13 — choix maximaliste assumé)*. Lexique + anecdotes + relations + historique indexé + UX de gestion + droits aidants + oubli. Conséquence : squad Langage renforcée sur P2 ; la spec détaillée du sous-système devient bloquante (D-5.6).
- **D-5.3 ✅ API du hub** *(2026-06-13, spec en §5.A)*.

### 5.A Spec — API du hub (D-5.3) *(renommée en review : elle couvre tout le hub, le moteur d'accélération en est le cœur)*

**Principes** : une **session = une conversation** avec KV-cache chaud côté hub ; **annulation par slot** (le débounce vit dans le serveur, pas dans chaque UI) ; streaming SSE pour les générations, WS pour l'événementiel ; tous les schémas dans `fluence-protocol`. Préfixe `/api/v1`.

```
POST   /pair                              // sans token, fenêtre d'appairage seulement (§2.A)
GET    /pair/info
POST   /sessions                          → {session_id}
DELETE /sessions/{id}
POST   /sessions/{id}/turns              {speaker: user|partner, text, source: typed|asr|spoken}
POST   /sessions/{id}/suggest            → SSE
GET    /sessions/{id}/next-chars?prefix= → {dist:[{ch,p},…]}
PUT    /sessions/{id}/draft              {text, caret}        // synchro du brouillon
POST   /memory/items                     {kind: person|place|routine|anecdote|phrase|fact,
                                          content, tags, acl}
GET    /memory/search?q=                 ; DELETE /memory/items/{id}
GET    /memory/pending                   ; POST /memory/pending/{id}/accept|reject   // §5.B
POST   /memory/forget                    {about}              // oubli sémantique journalisé
POST   /voice/speak                      {text, voice_id, prosody?} → audio streaming (Opus)
GET    /voice/voices
POST   /asr/listening                    {enabled, consent_token}   // cf. règle ci-dessous
GET/PUT /profiles/{id}                   // style, claviers, modalités, voix
PUT    /input/targets                    // cf. §4.A
GET    /system/health                    // état workers, modèles, latences p50/p95 glissantes
GET    /system/capabilities
POST   /openai/v1/chat/completions       // OPTIONNEL, off par défaut : proxy compat écosystème
```

**`/suggest`** — le cœur :
```jsonc
// Requête
{"mode":"replies"|"rephrase"|"expand"|"continue",
 "draft":"veu eau frache ce soir",      // tolérant au bruit de frappe oculaire
 "n":3, "slot":"main",                  // nouvelle requête même slot ⇒ annule la précédente
 "constraints":{"max_chars":120,"register":"famille"},
 "style_profile":"default"}
// Réponse SSE : delta {i,text} → final {suggestions:[{text,score}]} | aborted
```
- Les 4 modes partagent contexte, mémoire (retrieval automatique par tour, traçable en mode debug) et profil de style — seuls les prompts/LoRA diffèrent.
- **`next-chars`** : lecture des logits sur le KV chaud + agrégation BPE→caractères (sommer les probabilités des tokens candidats par premier caractère). Sert le dwell adaptatif, le scanning pondéré et Dasher. **Jamais** d'appel de génération complet sur ce chemin.
- **Consentement ASR de première classe** : `POST /asr/listening` exige un `consent_token` produit par une action UI explicite, journalisé, à TTL ; l'état d'écoute est diffusé sur le topic `system` (indicateur visible obligatoire sur toutes les UI).

**Budgets de latence** (sur FLU-REF-1 **en palier réduit/E2B** — cf. §3 et D-11.1 ; mêmes cibles en E4B sur FLU-REF-2/3 ; mesurés en CI) :
| Appel | p50 | p95 |
|---|---|---|
| `next-chars` (KV chaud) | 20 ms | 50 ms |
| `suggest` — premier delta affichable | 300 ms | 600 ms |
| `suggest` — 3 suggestions complètes | 1,2 s | 2,5 s |
| `speak` — premier échantillon audio | 200 ms | 400 ms |
| `turns` (indexation + re-warm KV en fond) | 100 ms | 250 ms |
| échantillon d'entrée → décision | 5 ms | 15 ms |
- **D-5.4 ✅ Structure du contexte** *(2026-06-13, spec en §5.C)*.
- **D-5.5 ✅ Pipeline de données** *(2026-06-13, spec en §5.D)*.
- **D-5.6 ✅ Sous-système Mémoire** *(2026-06-13, spec en §5.B)*.

### 5.B Spec — Sous-système Mémoire (D-5.6)

**Modèle de données** — items typés dans SQLite chiffré (SQLCipher) + index vectoriel local (sqlite-vec, embeddings du worker-embed) :
```jsonc
{"id":"…","kind":"person|place|routine|anecdote|phrase|fact|conversation_summary",
 "content":"Marie, ma fille — vient le mardi, deux enfants (Léo, Zoé)",
 "tags":["famille"], "source":"manual|learned|imported",
 "acl":"private|care_visible|care_editable",       // défaut : private
 "created_at":"…","last_used_at":"…","use_count":17,"confidence":0.9}
```

**Acquisition — trois canaux, l'agentivité d'abord :**
1. **Manuelle** : UI « Ma mémoire » (utilisateur, ou aidant selon ACL), édition au regard.
2. **Apprise** : après chaque conversation, un job local basse priorité extrait des candidats (personne nouvelle, phrase répétée ≥ 3×, anecdote racontée) → **file de validation** (`GET /memory/pending`, accept/reject/modifier). **Rien n'entre en mémoire apprise sans confirmation de l'utilisateur** — une mémoire suggérée, jamais imposée. Exception réglable : auto-accept des `phrase`.
3. **Import** : contacts (vCard/CSV) et calendrier (ICS), lus localement.

**Retrieval & injection** :
- À chaque tour : recherche hybride (BM25 + vectorielle, fusion RRF) sur le dernier tour de l'interlocuteur + le draft → top-6 au-dessus du seuil, filtrés par ACL → injectés compactés dans le prompt (« Mémoire : Marie = fille, vient le mardi… »). Boost récence/fréquence, décote des items anciens jamais utilisés.
- **Court-circuit `phrase`** : un match fort sur une formulation récurrente remonte directement en suggestion **sans appel LLM** (latence quasi nulle — la mémoire est aussi un cache de phrases).
- Budget : retrieval par tour < 30 ms (corpus personnel = petit). Transparence : les suggestions issues de la mémoire portent un marqueur discret ; en mode debug, `retrieved_items` est exposé.

**Permissions & oubli** :
- L'espace aidant ne voit **jamais** les items `private`, même en admin — anecdotes, faits intimes et conversations indexées sont `private` par défaut.
- Conversations brutes : purge automatique après 30 jours (configurable) ; seuls persistent les résumés et les items validés.
- `POST /memory/forget {about}` : recherche sémantique → liste de confirmation → suppression **journalisée en métadonnées seulement** (le journal d'oubli ne contient jamais le contenu oublié). Purge totale en un geste ; export JSON complet (portabilité).

### 5.C Spec — Structure du contexte (D-5.4)

**Assemblage du prompt — l'ordre est conçu pour maximiser le préfixe commun du KV-cache** (du plus stable au plus volatil) :
```
[1. System + tâche + style (stable par profil)]          ~350 tokens
[2. Mémoire injectée (semi-stable, change par tour)]     ~250 tokens
[3. Résumé glissant de la conversation ancienne]         ~200 tokens
[4. Tours récents datés (fenêtre)]                       ~1200 tokens
[5. Draft courant + instruction du mode]                 ~150 tokens
                                                  total  ≤ 2200 tokens
```
- **Fenêtre de tours** : 20 tours ou 1200 tokens (premier atteint). Au-delà : éviction vers le **résumé glissant**, mis à jour par le worker-llm en tâche de fond (priorité P3) — jamais sur le chemin chaud.
- **Datation relative** (« il y a 2 min », « hier ») et identité des locuteurs (« Marie (fille) : … ») dans les tours — le modèle doit savoir *qui* a dit *quoi* *quand*.
- **Situation** : bloc compact heure/jour/lieu-si-connu/interlocuteur-si-identifié. L'identification de l'interlocuteur en 1.0 = déclarative (l'utilisateur ou l'aidant sélectionne « je parle avec Marie ») ; reconnaissance du locuteur par la voix = 1.x (sujet privacy distinct).
- **Invalidation du KV-cache** : modification des blocs 1–3 → re-warm complet en fond ; ajout d'un tour (bloc 4) → append incrémental ; frappe dans le draft (bloc 5) → seul le suffixe est retraité. C'est ce qui rend `next-chars` < 30 ms possible.

### 5.D Spec — Pipeline de données (D-5.5)

**Étage 1 — Génération synthétique contrôlée** (`ml/data`) :
- Matrice de scénarios : **12 situations** (soins, repas, famille, rendez-vous médical, urgence, loisirs/TV, démarches admin, visite d'ami, téléphone, couple, aide à domicile, sortie) × **4 registres** (intime, familier, neutre, formel) × **profils de personnages** variés. Génération de dialogues complets multi-tours par grand modèle, avec consigne stricte anti-pathos : *vie ordinaire, humour, agacement, désirs — pas de misérabilisme* (les LLM caricaturent le handicap si on ne contraint pas ; c'est documenté dans la grille de relecture).
- Pour chaque tour « utilisateur » du dialogue, génération des **variantes d'entrée** : télégraphique (« eau frache stp »), bruitée (matrice de confusion spatiale du clavier AZERTY — les erreurs de frappe oculaire sont des voisins de touche, omissions, doublons), abrégée (règles FR à figer : initiales, squelette consonantique, abréviations usuelles SMS).
- **Étage 2 — Filtrage** : juges automatiques (naturalité, cohérence, diversité lexicale), déduplication par embeddings, détection de clichés sur le handicap. **Étage 3** : relecture humaine d'un échantillon ≥ 10 % avec grille publiée.
- **Corpus académiques (ESLO, ORFEO/CEFC, TCOF)** : tant que l'audit des licences n'autorise pas l'entraînement → usage en **calibration** (mesurer la distance distributionnelle synthétique ↔ oral réel, ajuster la génération) et en **évaluation**. Aucun corpus sous licence recherche dans les poids publiés.
- **Distillation LoRA** : teacher = grand modèle ; student = Gemma 4 E4B/E2B (QLoRA) ; un LoRA par tâche (`rephrase`, `replies`, `expand`) ; éval avant/après sur le harnais (§8.A), publication des deltas.
- **Hygiène** : datasets JSONL versionnés (DVC ou LFS), datasheet par dataset (provenance, biais connus), splits train/dev/test **gelés — le test ne sert jamais à itérer les prompts**.

---

## 6. Voix

### Acquis ✅
- **Deux régimes** : voix du quotidien en temps réel CPU (< 200 ms, tourne partout) ; pipeline intégré de **banque + clonage de voix** (enregistrement guidé proposé dès l'onboarding — quand la voix est encore là). *(Précisé en review : la voix personnelle du quotidien est un fine-tuning Piper, pas F5-TTS — cf. D-6.1.)*
- **Expressivité** : contrôle prosodique sélectionnable (question, rire, chuchotement, colère). L'expressivité est une demande utilisateur, pas un luxe.
- La voix clonée ne quitte jamais le foyer, chiffrée.

### Décisions
- **D-6.1 ✅ Architecture de clonage à deux étages** *(2026-06-13, spec en §6.A)*.
- **D-6.2 ✅ Protocole d'enregistrement** *(2026-06-13, spec en §6.A)*.
- **D-6.3 ✅ Prosodie v1** *(2026-06-13, spec en §6.A)*.

### 6.A Spec — Voix personnelle (D-6.1, D-6.2, D-6.3)

**D-6.1 — Deux étages, parce que la contrainte est l'inférence CPU :**
- **Étage quotidien : fine-tuning Piper (VITS)** sur le dataset de banque de voix. Entraînement : GPU requis **une fois** (PC familial avec GPU, machine d'un proche, ou service opt-in explicite — jamais silencieux) ; **inférence : CPU temps réel partout** (< 200 ms). C'est la décision de faisabilité : F5-TTS (diffusion) est trop lent sur CPU pour la conversation — une voix clonée qui ne tourne que sur GPU serait une fausse promesse pour le parc réel.
- **Étage qualité : F5-TTS (MIT)** en zero-shot/few-shot pour les paliers GPU et les usages différés où la latence importe peu : messages préparés, relecture d'anecdotes, messagerie. StyleTTS 2 écarté (anglo-centré) sauf démenti du benchmark FR.
- Sélection automatique par palier matériel (§3) + préférence utilisateur ; les deux étages consomment le **même dataset** d'enregistrement.

**D-6.2 — Protocole d'enregistrement (banque de voix) :**
- Proposé à l'onboarding avec délicatesse (formulation travaillée avec orthophonistes), refusable, **reprenable par sessions de ≤ 5 min** (fatigue). Cible : 30–60 min utiles ; un premier rendu écoutable dès ~10 min (gratification précoce, motivation).
- **Script de lecture en trois parts** : (1) phrases phonétiquement équilibrées FR (couverture diphones) ; (2) **lexique personnel** — prénoms des proches, lieux, expressions fétiches : les mots les plus dits doivent être les mieux rendus ; (3) **phrases émotionnelles jouées** (joie, tendresse, agacement, question, exclamation) — elles deviennent les références de style de l'étage F5-TTS.
- **Message banking en parallèle** : enregistrements bruts conservés tels quels (rires, « je t'aime », phrases rituelles avec la vraie prosodie) — déclenchables comme sons, jamais re-synthétisés.
- Guidage qualité en direct : VAD + SNR-mètre (« trop de bruit de fond »), re-prise en un geste, micro USB ~40 € recommandé (liste maintenue), pas de studio requis.
- **Voix déjà altérée** : mode « reconstruction » à partir d'archives (vidéos famille, messages vocaux) — pipeline denoise + séparation de sources, attentes honnêtement cadrées (qualité moindre, le produit le dit).

**D-6.3 — Prosodie v1 :**
- Jeu de tags : `neutre · question · exclamation · joie · tendresse · agacement · tristesse · chuchoté · fort · lent · rapide`.
- Réalisation : étage Piper = presets rate/pitch/volume par tag (léger, fiable) ; étage F5-TTS = **références audio émotionnelles** issues de l'enregistrement (la vraie expressivité de la personne, pas une émotion générique).
- UX : rangée de modificateurs à côté de PARLER (un commit = toute la phrase prend le tag) ; raccourcis dans le texte (`!!` → exclamation, `??` → question) ; mémorisation du tag par phrase en mémoire (§5.B).

---

## 7. Produit & UX

### Acquis ✅
- **Composeur de conversation texte-first** : zone de composition + barre de suggestions + clavier adapté ; affichage côté interlocuteur ; TTS immédiat.
- **Règles UX des suggestions** (le produit se gagne ici) : 3 suggestions max, positions stables, anti-scintillement (debounce + seuil de confiance), sélectionnables comme des touches, robustesse au texte bruité, instrumentation locale (WPM, frappes économisées en usage réel).
- **Adaptation à la progression** : mesure passive de la performance motrice, détection de dégradation, proposition de transition de modalité (regard → regard+tête → contacteur → scanning) **sans changer d'outil**.
- **Espace aidants/cliniciens** : configuration à distance consentie, tableaux de bord ergo (débit, fatigue, usage), profils types par pathologie/stade. Données stockées chez l'utilisateur.
- **Raccordements** : messageries (SMS/WhatsApp/Matrix/email), appels avec injection TTS, domotique via Home Assistant, alerte d'urgence. *(Phasage : voir D-7.4.)*
- **Grilles & import** : compatibilité OpenBoardFormat, import en un clic depuis Grid 3 / TD Snap / AsTeRICS Grid (arme d'adoption).
- L'outil lui-même est intégralement configurable au regard (accessibilité de la configuration).

### Décisions
- **D-7.1 ✅ Composeur & claviers** *(2026-06-13, spec en §7.A)*.
- **D-7.2 ✅ Onboarding** *(2026-06-13, spec en §7.B)*.
- **D-7.3 ✅ Espace aidant** *(2026-06-13, spec en §7.C)*.

### 7.A Spec — Composeur & claviers (D-7.1)

```
┌──────────────────────────────────────────────────────────────┐
│  DRAFT (texte en cours, curseur, très gros corps)        [⚙] │
├──────────────────────────────────────────────────────────────┤
│  [Suggestion 1 — la plus probable, la plus grande]           │
│  [Suggestion 2]                [Suggestion 3]                │ ← positions FIXES
├──────────────────────────────────────────────────────────────┤
│   a  z  e  r  t  y  u  i  o  p        ┌────────────┐         │
│   q  s  d  f  g  h  j  k  l  m        │   PARLER   │         │
│   ⇧  w  x  c  v  b  n  '  ⌫           │  (énorme,  │         │
│   [ESPACE────────────]  [.?!]         │ tjrs là)   │         │
│                                       └────────────┘         │
│  [Réponses] [Clavier] [Abrév.] [Mémoire]      [⚠ urgence]    │ ← onglets latéraux
└──────────────────────────────────────────────────────────────┘
```
- **Layout par défaut : AZERTY adapté** (la familiarité bat l'optimalité pour des adultes qui ont tapé toute leur vie) ; layouts alternatifs : fréquentiel FR (pour scanning à contacteur, où l'ordre = vitesse) et grille ABC. **Tailles de cibles adaptatives** : dimensionnées par le modèle de bruit utilisateur (§4.C) — le clavier de Marc (webcam) a de plus grosses touches que celui de Claire (IR).
- **Mode Réponses** : quand l'interlocuteur vient de parler (événement ASR), 3 réponses **plein écran** (cibles énormes, un commit = parlé). Retour au clavier d'un geste ou dès qu'on commence à taper.
- **Règles des suggestions (chiffrées)** : max 3, jamais réordonnées pendant qu'elles sont affichées, mise à jour au plus 1×/600 ms, **jamais pendant un dwell en cours > 40 %** (on ne vole pas une cible visée), seuil de confiance minimal sinon emplacement vide (un emplacement vide coûte moins qu'une mauvaise suggestion lue).
- **PARLER** : toujours même position, jamais masqué, gros ; double fonction relire/arrêter. **Urgence** : coin opposé, double confirmation (dwell long puis cible de confirmation — zéro faux positif), déclenche D-7.4.
- **Écran interlocuteur** (scope `display`) : texte énorme, historique court, indicateur « écrit… » ; **l'aperçu live du draft est un choix de l'utilisateur** (off par défaut — certains détestent être lus en cours de frappe ; agentivité).
- Toute l'UI (réglages compris) est navigable par les mêmes modalités — le composeur **consomme son propre protocole** (§4.A, dogfooding).
- Thèmes : contraste élevé AA+, sombre/clair, tailles ×1 à ×2 ; aucune information portée par la couleur seule.

### 7.B Spec — Onboarding (D-7.2)

Parcours « première heure » (objectif Sophie : **opérationnel en < 30 min**) :
1. **Détection matériel** (webcam, tracker souris, GPU, RAM) → palier (§3) + téléchargement des modèles en fond priorisé (on peut taper pendant que le LLM télécharge — D-2.6).
2. **Choix de la modalité** + **jeu de calibration** (§4.D, 45 s) + premier mot composé < 5 min (gratification immédiate).
3. **Voix** : choix d'une voix Piper FR **ou** proposition de banque de voix si la parole est encore présente (formulation co-écrite avec orthophonistes ; reportable, jamais insistant).
4. **Profil de style express** : 5 questions (tutoiement ? humour ? façon de dire oui/non ? expressions à toi ?) → alimente §5.C bloc 1.
5. **Import opt-in** : contacts/calendrier (§5.B) ; appairage du téléphone de l'aidant (scope `care`).
6. **Tutoriel interactif 10 min** : composer → suggestion → parler → réponse rapide. **Le mode abréviations n'est PAS dans l'onboarding** : proposé après 1 semaine d'usage réel (in-app, sur preuve d'aisance) — ne pas surcharger le jour 1.
- Sortie de l'onboarding : carte « si ça ne marche plus » imprimable pour l'aidant (1 page, 4 cas, gros caractères).

### 7.C Spec — Espace aidant (D-7.3)

- **Deux rôles** : aidant familial (`care`) et professionnel (`care` restreint, sous-ensemble choisi par l'utilisateur). **L'utilisateur voit et révoque tout** (liste des accès, journal des actions `care` — la confiance se construit par la transparence).
- **Fonctions** : santé du système (workers, batterie, qualité de calibration en temps réel §4.D) ; recalibration/réglages à distance (LAN) ; édition des raccourcis et grilles ; gestion des appareils appairés ; mises à jour (D-11.2) ; tableaux de bord — WPM, usage des modalités, tendance de fatigue (heuristique : vitesse déclinante intra-session), **uniquement en agrégats** (jamais le contenu des conversations — ACL §5.B).
- **Adaptation à la progression** (vue dédiée) : tendances motrices (vitesse de sélection, taux d'erreur, périodes d'usage) → suggestions de transition de modalité avec essai A/B guidé (« essayer le scanning 10 min ? »). Formulation produit : *« s'adapte à vos capacités du moment »* — jamais de langage médical prédictif (cohérence D-9.2).
- **Accès hors LAN : pas en 1.0** (décision : on n'ouvre pas le remote access avant un audit sérieux — le contournement temporaire est l'outil de prise en main générique choisi par la famille, hors de notre périmètre de responsabilité).
- Format : vues responsive du client web (le téléphone de l'aidant suffit).
- **D-7.4 ✅ Raccordements : tous dans la 1.0** *(2026-06-13 — maximalisme confirmé)*. Séquencement interne : **alerte d'urgence dès l'alpha P1** (simple et critique pour la confiance) ; **domotique Home Assistant en P2** ; **messageries (email/Matrix d'abord, SMS/WhatsApp ensuite) et appels avec injection TTS en P3**, avant la 1.0. Synergie à exploiter : l'audio de l'appel alimente le contexte ASR. ⚠️ Risque planning assumé : messageries (API WhatsApp restrictives) et téléphonie sont des chantiers épineux — premiers candidats à glisser en 1.1 si la 1.0 dérape.

---

## 8. Données & évaluation

### Acquis ✅
- **Le harnais de simulation est une infrastructure centrale, pas un outil annexe** : replay de dialogues FR, modèles d'utilisateurs synthétiques avec bruit moteur paramétrable, mesure des frappes économisées (KS%) et du WPM **en CI sur chaque commit**.
- **Benchmark public** d'accélération AAC multilingue (corpus, harnais, leaderboard) : leadership scientifique + boussole interne. Première évaluation à échelle du domaine (SpeakFaster : n=2).
- **Cibles chiffrées ✅** *(harmonisées en review avec §5.A ; mesurées sur les machines de référence D-11.1)* :
  - Suggestions : premier delta affichable p50 < 300 ms / p95 < 600 ms ; 3 complètes p50 < 1,2 s (§5.A).
  - TTS : premier échantillon < 200 ms. ASR : transcription partielle < 1 s après fin de tour.
  - Sélection regard webcam fusionné : ≥ 95 % de sélections correctes sur cibles de 2,5 cm à 60 cm.
  - KS% en simulation corpus FR v1 : **≥ 25 % sans contexte conversationnel (socle P1)** ; **≥ 40 % avec** (P2). ×3 WPM terrain à 12 mois ; 100 % des fonctions cœur offline.

### Décisions
- **D-8.1 ✅ Harnais de simulation** *(2026-06-13, spec en §8.A)*.

### 8.A Spec — Harnais de simulation (D-8.1)

**Deux niveaux d'exécution :**
1. **Offline pur** (`ml/eval`, par PR : sous-ensemble 50 dialogues ~5 min ; nightly : corpus complet 500+) — mesure la qualité linguistique, pas les latences.
2. **End-to-end** : le harnais pilote le **vrai hub** par l'API (§5.A) sur les machines de référence — latences réelles incluses, replay du protocole d'entrée (§4.A) compris. C'est le test d'intégration ultime : un dialogue rejoué de bout en bout, du regard simulé au TTS.

**Modèle d'utilisateur simulé** (le cœur méthodologique, hérité de Cai et al.) — paramètres versionnés :
- **Moteur moteur** : frappe lettre à lettre avec bruit (matrice de confusion spatiale, taux d'erreur 0–15 %), temps par sélection (modèle dwell : 600–1500 ms selon profil), fatigue progressive optionnelle (dégradation des paramètres au fil du dialogue).
- **Politique d'usage des suggestions** : consultation tous les k caractères (k=2–4), **coût de scan facturé** (350 ms + 150 ms/suggestion lue — pour modéliser le piège du coût de consultation), acceptation si la suggestion correspond à l'intention (similarité sémantique ≥ seuil avec la phrase cible du dialogue).
- **Modes** : lettre-à-lettre / +prédiction / +rephrase / +replies / +abréviations — chaque mode est une politique.

**Métriques produites** : KS% (frappes économisées vs lettre-à-lettre), **WPM simulé** (via le modèle temporel), taux d'acceptation des suggestions, taux de « suggestions nuisibles » (consultées mais inutiles = pur coût), précision des replies (similarité avec la vraie réponse du corpus), latences réelles (mode E2E).
**Baselines obligatoires** : lettre-à-lettre · n-gram de secours (notre fallback D-2.6) · moteur complet · **ablations** (sans mémoire / sans contexte interlocuteur / sans LoRA) — chaque gain revendiqué doit montrer son ablation.
**Portes de CI** : régression KS% > 2 points = échec ; régression latence p95 > 20 % = échec ; les chiffres nightly sont publiés (page de performance publique).
- **D-8.2 ✅ Sources de données FR : génération synthétique contrôlée + corpus oraux académiques** *(2026-06-13)*. Synthétique = socle des LoRA de tâche (scénarios paramétrés : registre, relation, situation ; filtrage par juges automatiques + relecture humaine). Corpus académiques (ESLO, ORFEO/CEFC, TCOF) = ancrage du registre oral réel — **audit des licences requis : usage en entraînement vs évaluation seule** (D-5.5). Collecte terrain et crowdsourcing : écartés à ce stade, réévaluables en 1.x avec protocole éthique dédié.
- **D-8.3 ✅ Benchmark public « FluenceBench-FR »** *(2026-06-13)*. Publication (repo dédié dans l'org GitHub) : sous-ensemble du corpus synthétique d'éval (**CC BY-SA 4.0** — possible car 100 % synthétique, aucune donnée personnelle), harnais open source (le même qu'en interne, mode offline), métriques standardisées (KS%, WPM simulé avec modèle d'utilisateur figé v1, taux d'acceptation), leaderboard par PR (résultats reproductibles exigés : script + seed). Les latences sont **hors classement** (incomparables entre machines) mais publiables à titre indicatif sur machine de référence. Extension multilingue du bench = même format, corpus par langue. Objectif secondaire assumé : devenir le point de référence académique du domaine en français.

---

## 9. Sécurité, vie privée, conformité

### Acquis ✅
- **Local-first chiffré comme seul design acceptable** (données de santé + voix + historique intime). Chiffrement au repos de tout ; aucune télémétrie par défaut ; export/portabilité complète ; suppression réelle.
- **Privacy des tiers** (micro) : opt-in par conversation, indicateur visible, traitement local, pas de conservation du brut.
- Qualification réglementaire : **traitée** (D-9.2 — aide technique, claims non médicaux). Stratégie remboursement (LPPR/PCH) : différée avec les trajectoires institutionnelles (D-12.3) ; le logiciel gratuit règle son propre coût, le financement du matériel conditionnera l'adoption à grande échelle.

### Décisions
- **D-9.1 ✅ Architecture des données** *(2026-06-13, spec en §9.A)*.
- **D-9.2 ✅ Position réglementaire : aide technique, claims non médicaux** *(2026-06-13)*. La 1.0 est une **aide technique à la communication** — aucun claim médical (jamais « mesure/suit la progression de la maladie » ; toujours « s'adapte à vos capacités du moment » — l'adaptation est une mesure d'usage, pas une mesure clinique). Disclaimer explicite : pas un dispositif médical. Si un jour des claims cliniques deviennent souhaitables (étude, remboursement) → trajectoire MDR avec partenaire institutionnel, pas avant. RGPD : tout est local, aucun compte, aucun traitement par le projet → le foyer reste maître de ses données ; notre obligation = privacy by design + documentation claire ; une DPIA ne devient nécessaire que si une télémétrie opt-in apparaît un jour.
- **D-9.3 ✅ Sécurité vérifiable, par étapes** *(2026-06-13)*. Dès P1 : `SECURITY.md` + politique de divulgation coordonnée, fuzzing des parsers réseau en CI (cargo-fuzz : protocole d'entrée, API, IPC), threat model publié (§9.A). Avant bêta publique : revue de sécurité communautaire organisée (annonce ciblée). Audit professionnel : quand des moyens existent (trajectoire D-12.3) — documenté comme dette assumée d'ici là.

### 9.A Spec — Architecture des données (D-9.1)

**Classification — chaque donnée a une classe, la classe dicte le traitement :**
| Classe | Exemples | Traitement |
|---|---|---|
| **P0 intime** | conversations, mémoire, voix clonée, enregistrements | chiffré au repos, **ne quitte jamais le foyer**, jamais dans les logs |
| **P1 personnel** | profils, calibrations, config | chiffré au repos, exportable |
| **P2 technique** | latences, état workers | local ; agrégats anonymes **opt-in** (off par défaut) |

- **Au repos** : bases SQLCipher (AES-256) pour store/mémoire ; blobs (datasets voix, modèles personnels) chiffrés (XChaCha20-Poly1305, crate `age`). **Clé maîtresse dans le keystore OS** (DPAPI Windows / Secret Service Linux) + **kit de secours imprimable** généré à l'installation (QR + phrase) — réalité du terrain : il faut pouvoir récupérer les données sur un autre PC quand celui-ci meurt.
- **Accès posthume/incapacité (« legacy access ») — opt-in explicite à l'onboarding, défaut NON** : l'utilisateur peut désigner un proche qui pourra déchiffrer (sa voix, ses textes) via le kit de secours. C'est un choix de la personne, jamais un défaut — la voix de quelqu'un ne s'hérite pas silencieusement.
- **Sauvegardes** : export chiffré planifié vers USB/NAS (jamais de cloud par défaut) ; **la restauration est testée en CI** (une sauvegarde non restaurable n'existe pas).
- **En transit** : §2.A. **Mémoire vive** : pas de zeroization systématique (hors scope), mais aucune donnée P0 dans les dumps de crash (rapports d'erreur expurgés par construction).
- **Threat model publié** (résumé) : couverts — voleur du PC (chiffrement), site web malveillant (anti drive-by §2.A), curieux du LAN (TLS + tokens), **aidant outrepassant** (ACL §5.B + journal) ; non couverts (documentés) — admin OS malveillant, attaquant physique persistant, États.



---

## 10. Écosystème & interopérabilité

### Acquis ✅
- Chaque couche publiée : protocole d'entrée ouvert ; moteur d'accélération en SDK TS/Python + endpoint OpenAI-compatible ; plugin AsTeRICS Grid officiel ; modèle de langage pour Dasher v6 ; formats ouverts (OpenBoardFormat, ARASAAC).
- Stratégie : capturer l'écosystème en le nourrissant (benchmark public, contributions upstream).

### Décisions
- **D-10.1 ✅ Licences par couche** *(2026-06-13)* : **briques réutilisables (input engine, moteur d'accélération, SDK, protocoles) en Apache-2.0** — adoptables par tout l'écosystème, y compris commercial, pour installer nos standards ; **application complète (composeur, hub assemblé, UX) en AGPL-3.0** — protège le produit du fork fermé.
- **D-10.2 ✅ Calendrier des intégrations** *(2026-06-12, amendé 2026-06-13 par D-7.4)* : plugin AsTeRICS Grid + modèle de langage Dasher v6 **dans la 1.0** (livrés en P2) ; **Home Assistant dans la 1.0** (P2) ; OptiKey-bridge en 1.x.
- **D-10.3 ⬜ Contact upstream AsTeRICS Grid** (mainteneur : UAS Technikum Wien) — à initier dès qu'une API est montrable (PLAN : piste parallèle, après Phase 4), pas au moment de la PR.
- **État vérifié (2026-06-13)** : la prédiction d'AsTeRICS Grid repose sur des dictionnaires fréquentiels auto-apprenants embarqués (predictionary, lz-string) — **aucun point d'extension de prédiction externe documenté**. Stratégie : PR upstream proposant une « external prediction source » (HTTP locale vers notre API §5.A) + POC court terme via leurs actions HTTP existantes. Notre API est conçue web-friendly pour ça (CORS origines appairées, latences §5.A). Côté **Dasher v6** (`dasher-web`, actif) : pas d'interface de modèle de langage stabilisée — notre `next-chars` est l'interface que nous proposerons (adaptateur `packages/integrations/dasher-lm`).

---

## 11. Ingénierie & qualité

### Acquis ✅
- **Ferme de matériel cible réel en CI** : les budgets de latence se mesurent sur le PC de 2019 et la tablette d'entrée de gamme.
- Tests : simulation (replay + utilisateurs synthétiques), E2E avec injection de regard simulé, fuzzing des drivers, benchs de latence en CI.
- Instrumentation locale consentie (les métriques restent chez l'utilisateur ; agrégats anonymes opt-in).

### Décisions
- **D-11.1 ✅ Machines de référence** *(2026-06-13)* — self-hosted runners CI, benchs nightly publiés :
| ID | Machine | Rôle |
|---|---|---|
| **FLU-REF-1** | laptop 2019, i5-8265U (4c/8t), 8 Go, SSD SATA, sans GPU, Windows 11 | **le plancher** — les budgets §5.A s'y mesurent (palier réduit) |
| **FLU-REF-2** | desktop 2022, Ryzen 5 + RTX 3060 12 Go, 16 Go, Windows 11 | le nominal GPU (palier hub) |
| **FLU-REF-3** | mini-PC N100, 16 Go, Linux | le hub économique (~250 €) |
| **FLU-REF-4** | tablette Windows d'occasion (Surface Go, 8 Go) | client web + webcam-only (persona Marc) |
- **D-11.2 ✅ Politique de release** *(2026-06-13)*. Canaux **beta** (continu) et **stable** (toutes les 6–8 semaines après bake en beta). **Jamais de mise à jour silencieuse sur stable** : proposée, notes en langage simple (« ce qui change pour vous »), reportable indéfiniment, **rollback en un geste** — l'outil de communication de quelqu'un ne change pas de comportement par surprise (cohérent avec D-3.2). Le client web suit la version du hub qui le sert (pas de désynchronisation possible). Versionnage SemVer ; LTS envisagé post-1.0 si le terrain le réclame.

---

## 12. Organisation & trajectoire

### Acquis ✅
- **Six squads** : Entrée (CV/tracking/fusion) · Langage (données, fine-tuning, éval, inférence) · Voix (TTS, clonage, prosodie) · Produit (UX accessibilité, composeur, aidants — ergothérapeute et designer spécialisé à demeure) · Plateforme (desktop/runtime, sécurité, CI, ferme) · Clinique & écosystème (études, partenariats, benchmark, communauté).
- **Phases** *(ajustées après D-12.1, 1.0 maximaliste)* : **P1 (0–6 mois)** fondations — harnais d'éval + corpus, runtime IA, Input Engine v1 (IR + webcam + fusion), accélération v1 (reformulation + prédiction), composeur + Piper, **alpha clinique sur le socle**. **P2 (6–12 mois)** le saut — ASR contextuel + réponses suggérées, RAG, voix personnelle, expansion d'abréviations, calibration continue durcie, intégrations Grid/Dasher, **bêta publique**. **P3 (12–18 mois)** — durcissement, raccordements (D-7.4), **release 1.0 publique (~12–15 mois)**, puis LoRA personnels, adaptation à la progression, multilingue, étude clinique publiée, dossier remboursement (18–24 mois).
- **Pérennité comme feature** *(trajectoires futures conditionnelles, cf. D-12.3 — aujourd'hui : projet GitHub)* : si le projet décolle — structure associative, financement mixte (NGI/NLnet, AFM-Téléthon, FIRAH), modèle de soutien type Nabu Casa (produit intégralement gratuit et libre ; services optionnels payants). En attendant : qualité de la doc, CI, architecture simple = la vraie assurance-vie du projet.
- Partenaires de validation visés (quand le produit le mérite) : ARSLA, centres de référence SLA, ergothérapeutes.

### Décisions
- **D-12.1 ✅ Contenu de la release 1.0 — maximaliste** *(2026-06-12)*. Socle (composeur texte + claviers adaptés ; entrée webcam/IR/contacteurs avec fusion, calibration continue, dwell adaptatif ; reformulation télégraphique + prédiction ; Piper FR ; espace aidant minimal ; harnais d'éval en CI) **+ les quatre extensions** : ASR contextuel + réponses suggérées · voix personnelle (clonage) · expansion d'abréviations (mode expert) · intégrations AsTeRICS Grid + Dasher v6. **Conséquences assumées** : la 1.0 publique glisse en fin de P2/début P3 (~12–15 mois) ; les six squads sont à pleine charge dès P1 ; le risque n°5 (dispersion) se contient par des jalons internes alpha/bêta stricts (D-12.2) et par la règle : *le socle atteint ses cibles chiffrées avant que les extensions ne soient marquées stables*.
- **D-12.2 ✅ Jalons à critères mesurables** *(2026-06-13)* — un jalon est franchi quand TOUS ses critères passent, mesurés par la CI :
  - **A1 — alpha (fin P1)** : socle complet sur FLU-REF-1 ; budgets §5.A tenus ; **KS% ≥ 25 %** (sans contexte) sur le corpus d'éval ; soak test 72 h sans crash ni fuite ; kill-tests (D-2.6) sans perte > 1 s ; installation < 30 min chrono par un tiers non préparé.
  - **B1 — bêta publique (fin P2)** : extensions 1.0 fonctionnelles sur paliers nominal+ ; **KS% ≥ 40 %** (avec contexte) ; pipeline voix personnelle complet (enregistrement → modèle → parole) ; intégrations Grid/Dasher en démo publique ; doc d'installation complète ; FluenceBench-FR publié.
  - **1.0** : ≥ 8 semaines de bêta ; zéro bug bloquant connu ; benchs publics au vert sur les 4 machines ; guide aidant + matrice trackers publiés ; vérifications nom/marque faites ; page de transparence sécurité (état D-9.3) en ligne.
- **D-12.3 ✅ Structure juridique : aucune pour l'instant** *(2026-06-13)*. Fluence est un projet open source publié sur GitHub, rien de plus. Les options institutionnelles (asso, financement NLnet/AFM, partenariats cliniques formels) restent documentées en §12 comme **trajectoires futures si le projet décolle** — pas des actions du présent. Conséquence : les « squads » décrivent une organisation du travail et un niveau d'exigence, pas un organigramme salarié.
- **D-12.4 ✅ Nom : Fluence** *(2026-06-13)*. Terme d'orthophonie (fluence verbale = fluidité de la parole), signifiant pour les cliniciens, prononçable dans toutes les langues cibles. À faire avant publication : vérification marques (INPI/EUIPO, collision connue : Renault Fluence, secteur éloigné), nom de l'organisation GitHub, domaines.

---

## Annexe A — Journal des décisions

| ID | Décision | Choix | Date | Statut |
|----|----------|-------|------|--------|
| D-2.1 | Forme de déploiement | Hybride app desktop + hub détachable (UI → hub via API réseau locale, un seul chemin de code) | 2026-06-12 | ✅ |
| D-2.2 | Plateformes natives 1.0 | Windows + Linux (macOS 1.x ; tablettes via clients web) | 2026-06-12 | ✅ |
| D-2.3 | Stack | Rust (hub) + TypeScript (UI/SDK) + Python (ML offline) | 2026-06-12 | ✅ |
| D-12.1 | Périmètre 1.0 | Maximaliste : socle + ASR/réponses + voix personnelle + abréviations + intégrations Grid/Dasher ; 1.0 à ~12–15 mois | 2026-06-12 | ✅ |
| D-10.2 | Calendrier intégrations | Grid + Dasher dans la 1.0 ; OptiKey-bridge & Home Assistant en 1.x | 2026-06-12 | ✅ |
| D-3.1 | Politique cloud | Local par défaut + opt-in granulaire par fonction, réversible, même protocole | 2026-06-13 | ✅ |
| D-5.1 | Stratégie modèle 1.0 | Prompts + LoRA de tâche distillés ; hypothèse Gemma 4 E4B (Apache 2.0), benchmark tranche | 2026-06-13 | ✅ |
| D-5.2 | Mémoire personnelle | Complète dès la 1.0 (lexique + anecdotes + relations + historique) ; D-5.6 bloquante pour P2 | 2026-06-13 | ✅ |
| D-8.2 | Sources données FR | Synthétique contrôlé + corpus oraux académiques (audit licences) ; terrain/crowdsourcing écartés | 2026-06-13 | ✅ |
| D-10.1 | Licences | Briques Apache-2.0 · application AGPL-3.0 | 2026-06-13 | ✅ |
| D-7.4 | Raccordements | Tous en 1.0 : urgence (P1), Home Assistant (P2), messageries + appels TTS (P3) ; premiers candidats au glissement 1.1 | 2026-06-13 | ✅ |
| D-12.3 | Structure juridique | Aucune pour l'instant — projet open source GitHub ; trajectoires institutionnelles différées | 2026-06-13 | ✅ |
| D-12.4 | Nom | **Fluence** (vérifs marques/domaines avant publication) | 2026-06-13 | ✅ |
| D-2.4 | Topologie & sécurité réseau | Loopback :7411 sans TLS / LAN opt-in HTTPS + mDNS + appairage par code ; tokens par appareil à scopes (display/control/care/system) ; CORS allowlist anti drive-by | 2026-06-13 | ✅ |
| D-2.5 | Organisation du code | Monorepo (crates/apps/packages/ml) ; fluence-protocol source de vérité des schémas (Rust→JSON Schema→OpenAPI+TS) | 2026-06-13 | ✅ |
| D-2.6 | Fiabilité | « Le clavier parle toujours » : workers IA en processus isolés, chaîne de dégradation explicite (n-gram embarqué, voix OS de repli), démarrage < 3 s, autosave ≤ 1 s | 2026-06-13 | ✅ |
| D-4.1 | Protocole d'entrée | FluenceInput v1 : sources→moteur de sélection (hub)→événements ; cibles déclarées par l'UI, hit-testing/dwell côté hub ; UDP compat Opentrack | 2026-06-13 | ✅ |
| D-5.3 | API du hub | Sessions à KV chaud, /suggest SSE avec annulation par slot, /next-chars < 30 ms, consentement ASR de première classe, budgets de latence contractuels | 2026-06-13 | ✅ |
| D-5.6 | Sous-système Mémoire | SQLCipher + sqlite-vec ; acquisition manuelle/apprise (file de validation obligatoire)/import ; retrieval hybride < 30 ms ; court-circuit phrases sans LLM ; ACL private/care ; oubli journalisé en méta | 2026-06-13 | ✅ |
| D-1.1 | Personas | Claire (SLA, IR, conversationnel), Marc (locked-in, webcam-only, palier réduit), Sophie (ergo, < 30 min) + Jean (aidant) | 2026-06-13 | ✅ |
| D-3.2 | Gestion des modèles | Manifestes signés (minisign), stabilité comportementale (jamais de MAJ silencieuse), pack USB hors-ligne | 2026-06-13 | ✅ |
| D-3.3 | Scheduler | Priorités strictes P0 TTS > P1 suggest > P2 ASR > P3 fond ; préemption ; budgets par palier ; pas de swap silencieux | 2026-06-13 | ✅ |
| D-4.2 | Stratégie trackers | 4 niveaux : souris universelle / protocoles ouverts (OpenGaze, Opentrack, FluenceInput-UDP) / webcam native / SDK après audit — aucun SDK propriétaire prérequis | 2026-06-13 | ✅ |
| D-4.3 | Fusion v1 | Prétraitement → One Euro → I-VT fixations → fusion pondérée + « regard désigne, tête affine » → magnétisme linguistique plafonné 40 % ; modèle de bruit continu par utilisateur | 2026-06-13 | ✅ |
| D-4.4 | Calibration | Initiale smooth pursuit 45 s ; continue implicite sur commits non corrigés ; détection de dérive → express 3 pts/10 s ; profils de contexte ; qualité visible | 2026-06-13 | ✅ |
| D-5.4 | Structure du contexte | Prompt ≤ 2200 tokens ordonné stable→volatil pour le KV-cache ; fenêtre 20 tours ; résumé glissant en P3 ; datation relative ; interlocuteur déclaratif en 1.0 | 2026-06-13 | ✅ |
| D-5.5 | Pipeline de données | 12 situations × 4 registres ; variantes télégraphique/bruitée/abrégée ; consigne anti-pathos ; corpus académiques = calibration/éval seulement ; QLoRA par tâche ; splits gelés | 2026-06-13 | ✅ |
| D-6.1 | Clonage deux étages | Quotidien = fine-tuning Piper (inférence CPU) ; qualité/différé = F5-TTS (GPU) ; même dataset | 2026-06-13 | ✅ |
| D-6.2 | Enregistrement voix | Sessions ≤ 5 min, cible 30–60 min, rendu dès ~10 min ; script 3 parts (phonétique + lexique personnel + émotions) ; message banking ; mode reconstruction d'archives | 2026-06-13 | ✅ |
| D-6.3 | Prosodie v1 | 11 tags ; Piper = presets, F5-TTS = références émotionnelles de la personne ; raccourcis !!/?? | 2026-06-13 | ✅ |
| D-7.1 | Composeur | AZERTY adapté par défaut, cibles adaptatives (modèle de bruit), 3 suggestions positions fixes, règles anti-vol de dwell, PARLER invariant, urgence double confirmation, aperçu interlocuteur off par défaut | 2026-06-13 | ✅ |
| D-7.2 | Onboarding | < 30 min : matériel→palier, calibration-jeu, voix (banque proposée avec délicatesse), style express, tutoriel 10 min ; abréviations à J+7, pas au jour 1 | 2026-06-13 | ✅ |
| D-7.3 | Espace aidant | Rôles care familial/pro ; l'utilisateur voit et révoque tout ; agrégats jamais le contenu ; progression en langage non médical ; pas de remote hors LAN en 1.0 | 2026-06-13 | ✅ |
| D-8.1 | Harnais de simulation | Offline (PR/nightly) + E2E sur vraies machines ; utilisateur simulé avec coût de scan facturé ; baselines + ablations obligatoires ; portes CI (KS% −2 pts, latence +20 %) | 2026-06-13 | ✅ |
| D-8.3 | FluenceBench-FR | Corpus synthétique CC BY-SA, harnais public, leaderboard par PR reproductible, latences hors classement | 2026-06-13 | ✅ |
| D-9.1 | Architecture données | Classes P0/P1/P2 ; SQLCipher + age ; clé en keystore OS + kit de secours imprimable ; legacy access opt-in (défaut non) ; restauration testée en CI ; threat model publié | 2026-06-13 | ✅ |
| D-9.2 | Réglementaire | Aide technique, claims non médicaux (« capacités du moment ») ; RGPD : tout local, pas de traitement par le projet | 2026-06-13 | ✅ |
| D-9.3 | Sécurité vérifiable | SECURITY.md + fuzzing CI dès P1 ; revue communautaire avant bêta ; audit pro = dette documentée | 2026-06-13 | ✅ |
| D-11.1 | Machines de référence | FLU-REF-1 (plancher 2019/8 Go), -2 (RTX 3060), -3 (N100 Linux), -4 (tablette webcam) ; benchs nightly publiés | 2026-06-13 | ✅ |
| D-11.2 | Releases | beta continu / stable 6–8 sem ; jamais silencieux sur stable, rollback en un geste ; client web suit le hub | 2026-06-13 | ✅ |
| D-12.2 | Jalons | A1 : KS ≥ 25 % + soak 72 h + install < 30 min ; B1 : KS ≥ 40 % + voix complète + bench publié ; 1.0 : 8 sem de bêta + 4 machines au vert | 2026-06-13 | ✅ |
| review | Corrections 2026-06-13 | Flotte par paliers (8 Go ≠ 8 Go RAM) ; IPC named pipes Windows ; /pair sans token mais fenêtre d'appairage ; API « du hub » ; cibles §8 harmonisées §5.A | 2026-06-13 | ✅ |

## Annexe B — Risques majeurs & mitigations ✅

1. **Qualité FR insuffisante des modèles ~4B sur la tâche** → le harnais d'éval tranche dès la Phase 3–4 du PLAN ; plans B : LoRA distillé, 7–8B en mode hub, exigence matérielle assumée.
2. **Latence sur le parc réel** → cibles chiffrées mesurées en CI (seuils provisoires GitHub, puis contractuels sur machines de référence dès la Phase 7).
3. **Dépendance aux upstreams** (Grid, SDK trackers) → produit autonome d'abord, intégrations en parallèle ; stratégie trackers sans SDK propriétaire (D-4.2) ; contact mainteneurs quand l'API est montrable (D-10.3).
4. **Accès aux utilisateurs** (recrutement, éthique) → la simulation porte le projet (Phase 3) ; contact terrain (ARSLA, ergothérapeutes) quand l'alpha A1 est montrable — pas de promesse institutionnelle avant (D-12.3).
5. **Dispersion** → périmètre 1.0 verrouillé par ce document ; tout le reste est extension datée.
6. **Pérennité/abandon** (cimetière du secteur) → aujourd'hui : doc/CI/architecture simple comme assurance-vie (cf. D-12.3 — pas de structure) ; trajectoires institutionnelles documentées en §12, à activer si le projet décolle.

## Annexe C — Références

- `Project.md` — rapport de recherche source (cartographie, briques, recommandation initiale).
- Littérature clé : Cai et al. 2022 (arXiv:2205.03767) & 2024 (*Nature Comms*, SpeakFaster) ; KWickChat (IUI 2022) ; Valencia et al. (CHI 2023, agentivité AAC) ; WebEyeTrack (arXiv:2508.19544). *Références à re-sourcer précisément en phase de vérification.*
- Briques : llama.cpp, whisper.cpp, Voxtral Realtime (Mistral), Piper, F5-TTS, StyleTTS 2, MediaPipe, ONNX Runtime, AsTeRICS Grid, Dasher v6 (`dasher-web`), OpenBoardFormat.
- Vérifications web 2026-06-13 : [Gemma 4 (annonce Google)](https://blog.google/innovation-and-ai/technology/developers-tools/gemma-4/) · [model card Gemma 4](https://ai.google.dev/gemma/docs/core/model_card_4) · [structure AsTeRICS Grid](https://www.asterics.eu/develop/asterics-grid/01_structure.html) · [dasher-web](https://github.com/dasher-project/dasher-web) · [comparatif Voxtral/Whisper 2026](https://weesperneonflow.ai/en/blog/2026-03-31-voxtral-whisper-open-source-speech-models-comparison-2026/) · [benchmarks STT open source 2026](https://northflank.com/blog/best-open-source-speech-to-text-stt-model-in-2026-benchmarks).
