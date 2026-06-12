# PLAN — Fluence : exécution jusqu'au jalon A1 (et esquisse au-delà)

> **Rôle de ce document** : le guide d'exécution opérationnel. `SPEC.md` dit *quoi et pourquoi* ; ce plan dit *dans quel ordre, avec quels tests, et quand c'est fini*. Il est conçu pour qu'une session de travail puisse reprendre n'importe où : regarder §4 (état), ouvrir la phase courante, exécuter.
> **Repo** : `git@github.com:Pluenet-Killian/Fluence.git` · **Méthode** : re-détaillage glissant — les phases jusqu'à A1 sont ultra-précises, la suite est esquissée et sera détaillée à l'approche.
> **Tailles** : S ≈ 1–3 sessions · M ≈ 3–8 · L ≈ 8–15 · XL = piste longue parallèle.
> **Vocabulaire** (ne jamais confondre) : **Phase 0–7** = étapes d'exécution de CE plan · **P1/P2/P3** = phases produit de la SPEC §12 (les Phases 0–7 vivent dans P1) · **A1/B1/1.0** = jalons (SPEC D-12.2) · **D-x.y** = décisions de la SPEC.
> **Version** : 1.1 — 2026-06-13.

---

## 0. Règles d'exécution (invariantes, toutes phases)

1. **Definition of Done universelle** — une tâche est finie quand : code + tests du comportement nouveau + doc (rustdoc/tsdoc/docstring sur API publique) + CI verte sur Windows ET Linux + self-review faite (+ `/code-review` sur les PR substantielles). Pas d'exception « c'est trivial ».
2. **Bug = test d'abord** : tout bug corrigé commence par un test rouge qui le reproduit (le commit montre rouge→vert).
3. **Chaque phase se termine par une passe de consolidation** : refactor de ce qui a grincé, dette notée en issues étiquetées `debt`, ADR à jour, CHANGELOG. On ne construit pas la phase N+1 sur du sable.
4. **Git** : trunk-based. `main` protégée (CI verte requise, force-push interdit). Branches courtes `feat/...`, `fix/...`, PR squash-mergées, conventional commits (vérifiés par hook). Tags `phase-N-done` en fin de phase.
5. **Décisions** : toute décision d'architecture prise en cours de route = un ADR dans `docs/adr/NNNN-titre.md` (template fourni en phase 0). La SPEC reste la source de vérité produit ; un conflit spec↔réalité se résout par amendement explicite de la SPEC, jamais en silence.
6. **Qualité Rust** : `#![deny(warnings)]` en CI, clippy `pedantic` (allowlist d'exceptions justifiées), `unsafe` interdit hors crates `-sys`/FFI isolées (vérifié par `#![forbid(unsafe_code)]` ailleurs), `cargo-deny` (licences + advisories) bloquant, MSRV épinglée. **TS** : eslint strict + `tsc --noEmit` bloquants, pas de `any` non justifié. **Python** : ruff + mypy strict, pydantic v2 pour tout format de données.
7. **Logs** : `tracing` structuré ; **aucune donnée P0 dans les logs** (§9.A SPEC) — règle vérifiée par revue + test dédié dès la phase 2.
8. **Budgets de latence** : deux régimes de seuils — `provisional` (runners GitHub, multiplicateur ×2,5, non-bloquant en PR mais tracé) et `contractual` (self-hosted FLU-REF, bloquant) ; bascule en phase 7. Les chiffres contractuels sont ceux de SPEC §5.A.
9. **Rituel de session IA** : chaque session de code **commence** par lire `CLAUDE.md`, l'état (§5 de ce plan) et la phase courante ; elle **se termine** en mettant à jour §5 (cases « Done quand » cochées, dette notée en issues, déviations consignées). Une session qui ne met pas à jour l'état n'a pas terminé.

---

## 1. Stratégie de test — la pyramide T1→T6

| Niveau | Quoi | Outils | Quand |
|---|---|---|---|
| **T1 Unit** | logique pure par crate/package, doctests | cargo test, vitest, pytest | chaque PR, < 5 min |
| **T2 Propriétés & fuzz** | invariants (sommes de probas ≈ 1, coordonnées ∈ [0,1], idempotences) ; parsers (WS, IPC, UDP, BPE) | proptest ; cargo-fuzz (corpus minimisés commités) | propriétés : chaque PR · fuzz : nightly 10 min/cible |
| **T3 Contrats** | schémas JSON/OpenAPI = golden files ; round-trip serde ; le SDK TS généré compile et ses tests de types passent | xtask check-contracts, openapi-typescript | chaque PR — un schéma qui bouge sans mise à jour des goldens = rouge |
| **T4 Intégration** | hub réel + workers (factices ou tiny-model), kill-tests, appairage, scopes, SSE/annulation | tests Rust d'intégration pilotant le binaire | chaque PR, < 10 min |
| **T5 E2E produit** | scénarios personas dans le vrai client web + vrai hub (tiny-LLM, Piper réel) | Playwright | chaque PR (suite courte) + nightly (complète) |
| **T6 Valeur & perf** | harnais d'éval (KS%, WPM simulé) = non-régression de la VALEUR ; benchs de latence à seuils ; soak | ml/eval + criterion + benchs custom `assert_latency` | PR : sous-ensemble 20 dialogues · nightly : corpus complet + soak + vrais modèles |

**Modèles en CI** — règle stricte pour rester rapide et hermétique :
- **Tiny-LLM de test** (~100–300 Mo GGUF, ex. SmolLM2-135M-Instruct) : valide la *mécanique* (sessions, KV, SSE, annulation, next-chars), jamais la qualité. Caché par les runners.
- **Piper** + voix `fr_FR-siwis-medium` (~60 Mo) : réel dès la CI (il est assez léger).
- **Vrais modèles** (Gemma 4 E2B/E4B) : nightly sur self-hosted uniquement (qualité + latences contractuelles).
- Tests LLM déterministes : seed fixé + décodage greedy pour les assertions exactes ; les évaluations qualité utilisent des seuils statistiques (n runs), jamais d'assertion exacte sur du sampling.
- Fixtures (traces d'entrée, audio, dialogues) : versionnées en Git LFS, petites (< 5 Mo chacune).

**Couverture** (gates CI, mesurée cargo-llvm-cov / vitest --coverage) : `fluence-protocol`, `fluence-input`, `fluence-accel`, `fluence-store` ≥ 85 % lignes · `fluence-hub` ≥ 75 % · `@fluence/sdk` ≥ 80 %. Pas de gate sur les UI (couvertes par T5). La couverture ne remplace jamais un test pensé — elle détecte l'oubli.

---

## 2. Phases détaillées (P1 → jalon A1)

### Phase 0 — L'usine (repo, CI, qualité) — **M**

**Objectif** : un monorepo où *tout* est vérifié automatiquement avant d'écrire la première vraie feature.
**In** : structure §2.B de la SPEC, outillage, CI, gouvernance du code. **Out** : tout code fonctionnel.

**Tâches**
- 0.1 Init repo : arborescence SPEC §2.B + `xtask/` (tâches repo en Rust : `check-contracts`, `download-test-assets`, `run-eval`) + `docs/` (y verser `SPEC.md`, `PLAN.md`, `Project.md`) + templates (PR, issues, ADR-0000-template) + `CONTRIBUTING.md`, `SECURITY.md` (D-9.3), `CODE_OF_CONDUCT.md`.
- 0.2 Licences (D-10.1) : Apache-2.0 à la racine de `crates/` et `packages/` (briques), AGPL-3.0 sur `apps/` ; en-têtes vérifiés par `xtask` (ou REUSE) ; `cargo-deny` configuré.
- 0.3 Workspaces : Cargo workspace (resolver 2, profils release/dev, MSRV) · pnpm workspace (TS strict, eslint flat, vitest) · uv (ruff, mypy, pytest). Crates/packages « hello » avec un test chacun, pour valider la tuyauterie.
- 0.4 Hooks locaux (lefthook) : fmt + clippy (fichiers touchés) + eslint + ruff + typos + commitlint — budget < 5 s (un hook lent finit désactivé).
- 0.5 CI GitHub Actions : `ci.yml` (lint+T1+T3, matrice Win+Linux, caches) · `integration.yml` (T4+T5) · `nightly.yml` (T2 fuzz, T6, soak) · `release.yml` (dry-run de packaging). Protections de branche actives.
- 0.6 ADR-0001 « architecture hub/workers » + ADR-0002 « monorepo & contrats » (recopie des décisions SPEC, pour que le repo soit auto-porteur).
- 0.7 `CLAUDE.md` à la racine (fourni — il cadre chaque session IA : pointeurs SPEC/PLAN, règles non négociables, rituel de session) + versement de `SPEC.md`, `PLAN.md`, `Project.md` dans `docs/`.

**Tests & vérifications — « l'usine se teste »** (exécutés une fois, documentés dans la PR de phase)
- PR-cobaye avec lint cassé → CI rouge. PR avec commit non conventionnel → hook + CI rouges.
- `cargo build --workspace && cargo test` + `pnpm -r build && pnpm -r test` + `uv run pytest` verts sur Win + Linux.
- Dépendance à licence interdite ajoutée → `cargo-deny` rouge.

**Done quand**
- [ ] CI verte de bout en bout sur les deux OS depuis un clone frais (vérifié dans un répertoire vierge).
- [ ] Les 4 workflows existent et ont tourné au moins une fois (dont release dry-run).
- [ ] `docs/` contient SPEC, PLAN, ADR-0001/0002 ; badges README (CI, licences).

---

### Phase 1 — Le contrat (`fluence-protocol` + SDK généré) — **M**

**Objectif** : tous les types de la SPEC (§4.A, §5.A, §5.B, §2.A) définis une fois, générés partout — la dérive de contrat devient impossible.
**In** : types Rust + schemars → JSON Schema → OpenAPI 3.1 → types TS ; erreurs problem+json ; scopes. **Out** : toute implémentation des endpoints.

**Tâches**
- 1.1 Types FluenceInput v1 : `PointerSample`, `SwitchEvent`, `TargetMap`/patchs, `sel.*`, `scan.*` (+ invariants : coordonnées [0,1], timestamps µs monotones par source).
- 1.2 Types API hub : sessions, turns, suggest (req + événements SSE), next-chars, draft, memory (items, pending, forget, ACL), voice, asr (consent), profiles, pair, health, capabilities ; enveloppe d'erreur RFC 9457 avec catalogue de codes stables.
- 1.3bis **Niveaux de stabilité dans le contrat** : les domaines du socle A1 (input, sessions/suggest/next-chars, voice de base, pair, system) sont marqués `stable` ; les domaines P2 (memory avancée, asr, voice clonage) sont définis mais marqués `experimental` (`x-fluence-stability` dans l'OpenAPI, `#[non_exhaustive]` côté Rust) — on fige tôt ce qu'on implémente tôt, on garde le droit d'apprendre sur le reste.
- 1.3 Chaîne de génération : `xtask check-contracts` — schemars → `schemas/*.json` (goldens commités) → OpenAPI 3.1 → `packages/sdk/src/generated/`. CI échoue sur tout diff non commité.
- 1.4 `@fluence/sdk` v0 : client typé minimal (fetch + SSE + WS), zéro logique métier, tests de types (expect-type) + mocks.

**Tests**
- T1 : sérialisation/désérialisation de chaque type avec fixtures lisibles (les fixtures servent de doc).
- T2 proptest : round-trip serde pour tout type ; rejet des hors-bornes (x=1.2, scope inconnu…).
- T3 : goldens schémas ; `tsc` sur le SDK généré ; OpenAPI validé (spectral lint).

**Done quand**
- [ ] 100 % des messages/endpoints des §4.A/§5.A existent en Rust + JSON Schema + TS, goldens verrouillés.
- [ ] Doc API auto-générée publiée (GitHub Pages, job CI).
- [ ] Couverture protocol ≥ 85 %.

---

### Phase 2 — Hub & supervision : « le clavier parle toujours » — **L**

**Objectif** : le squelette vital de D-2.6 — un hub qui démarre vite, s'appaire, supervise des workers, survit à tout. **Aucune IA réelle ici** : workers factices (echo) pour prouver la mécanique.
**In** : axum HTTP+WS, supervision, IPC, store chiffré, pair/tokens/scopes/CORS, `/system/health`, autosave draft. **Out** : LLM, ASR, TTS réels (phases 4–5).

**Tâches**
- 2.1 `fluence-hub` : bootstrap (config TOML + env, port 7411, repli), `tracing` avec couche de **redaction P0** (champ marqué sensible → jamais loggé), arrêt propre.
- 2.2 Couche IPC : abstraction UDS (Linux) / named pipes (Windows), messages JSON préfixés longueur, heartbeat worker, harness de worker factice (`worker-echo`) pour tous les tests.
- 2.3 Superviseur : spawn/health/restart avec backoff exponentiel + jitter, états (`starting/ready/degraded/down`), événements `system.degraded` sur le topic WS `system`.
- 2.4 `fluence-store` : SQLCipher (clé via keyring OS ; mode test : clé fichier), migrations (refinery/sqlx-migrate), tables profils + drafts + journal d'accès ; autosave draft (write-ahead, débounce 500 ms).
- 2.5 Appairage & sécurité (§2.A) : fenêtre d'appairage (2 min, code usage unique, rate-limit), tokens par appareil + scopes, CORS allowlist, middleware d'auth ; mode foyer : TLS (rcgen CA locale) + mDNS (annonce) — *l'épinglage côté client attend la phase 5*.
- 2.6 `/system/health` + `/system/capabilities` + `fluencectl` v0 (`health`, `pair`, `watch`).

**Tests**
- T1 : tokens/scopes (matrice complète accès×scope), fenêtre d'appairage (expiration, rejeu, brute-force → 429), backoff calculé, redaction (un log contenant un champ P0 marqué = test rouge).
- T2 fuzz : parseur IPC + frames WS (cargo-fuzz, premières cibles).
- T4 **kill-tests** (le cœur de la phase) : SIGKILL/TerminateProcess d'un worker → `system.degraded` émis < 500 ms, relance avec backoff, compteur de restarts exposé ; kill du hub pendant frappe simulée → au redémarrage le draft restauré (perte ≤ 1 s, mesurée par horodatage) ; 50 cycles kill/restart sans fuite (RSS stable ±10 %).
- T4 : requête sans token → 401 uniforme ; origine non appairée → CORS bloqué ; `GET /pair/info` seul accessible.
- T6 : démarrage hub → ready **< 3 s** (provisional ×2,5 en CI GitHub) ; soak court nightly : 30 min de churn (appairages, WS connect/disconnect, kills aléatoires d'echo-workers) sans fuite fd/RSS.

**Done quand**
- [ ] Démo scriptée : `fluencectl pair` depuis une 2e machine (ou conteneur) + kill -9 du worker echo en boucle → le hub reste sain, les événements arrivent.
- [ ] Tous les kill-tests passent sur Win + Linux en CI.
- [ ] Journal d'accès consultable ; zéro P0 dans les logs (test + revue).

---

### Phase 3 — La boussole (harnais d'éval + corpus v0 + baselines) — **L**

**Objectif** : mesurer avant de construire — le harnais §8.A opérationnel avec deux baselines, branché en CI. Sous-produit : le **n-gram FR de secours** (fallback D-2.6 ET baseline).
**In** : ml/eval + ml/data v0, n-gram crate. **Out** : LLM (phase 4), benchmark public (post-A1).

**Tâches**
- 3.1 Formats (pydantic) : dialogue JSONL (scénario, registre, tours, vérités terrain), résultat d'éval (métriques par dialogue + agrégats), versionnés.
- 3.2 Modèle d'utilisateur simulé v0 : frappe + matrice de confusion AZERTY (voisinage spatial) + modèle temporel (dwell 600–1500 ms paramétré) + politique de consultation des suggestions avec **coût de scan facturé** (350 ms + 150 ms/suggestion) + acceptation par similarité sémantique (embeddings locaux) ≥ seuil. **Les politiques par mode (lettre-à-lettre, +prédiction, +rephrase) existent dès v0** — la phase 4 mesure rephrase dès son premier jour.
- 3.3 Métriques : KS%, WPM simulé, taux d'acceptation, taux de suggestions nuisibles — chacune testée sur des cas construits à la main aux valeurs attendues exactes.
- 3.4 Corpus synthétique v0 : ~100 dialogues (12 situations × registres réduits), consigne anti-pathos, dédup embeddings, relecture manuelle (toi + moi), splits gelés dev/test.
- 3.5 `fluence-ngram` (crate Rust + binding eval) : modèle fréquentiel FR compact (listes de fréquences libres + corpus synthétique train), < 10 Mo, API = celle d'une source de prédiction.
- 3.6 Intégration CI : `xtask run-eval --suite pr` (20 dialogues, < 5 min) par PR avec publication du delta en commentaire ; suite complète nightly + page de résultats.

**Tests**
- T1 : métriques exactes sur cas construits ; déterminisme (même seed → mêmes chiffres au bit près).
- T1 : matrice de confusion — propriétés (voisins plus probables que lointains, somme = 1).
- T6 sanity : baseline n-gram **bat** lettre-à-lettre (KS% > 0) ; un « moteur oracle » (qui connaît la cible) atteint KS% ≈ borne haute — encadrement qui valide le harnais lui-même.
- Qualité corpus : grille anti-pathos appliquée à 100 % (juge auto + relecture), stats publiées (longueurs, diversité lexicale).

**Done quand**
- [ ] `xtask run-eval` produit un rapport reproductible ; CI commente les PR avec le delta KS%.
- [ ] Encadrement oracle/n-gram/lettre-à-lettre cohérent et documenté.
- [ ] Le n-gram est packagé comme source de prédiction utilisable par le hub (prêt pour le fallback).

---

### Phase 4 — Le moteur (worker-llm + accélération v0) — **L**

**Objectif** : `rephrase` + `continue` + `next-chars` réels, sessions à KV chaud, annulation par slot — et un **premier chiffre de valeur** au harnais.
**In** : worker-llm (llama.cpp), fluence-accel, téléchargement de modèles v0. **Out** : replies/ASR (P2), abréviations (P2), LoRA (piste ML parallèle).

**Tâches**
- 4.1 ADR-000X : intégration llama.cpp — binding crate (llama-cpp-2) vs FFI maison vs sous-processus llama-server ; critères : contrôle du KV par session, accès aux logits (next-chars), stabilité Windows. Implémentation derrière un trait `LlmBackend` (le backend OpenAI-compatible distant — D-3.1 — implémente le même trait, en stub testé dès maintenant).
- 4.2 `worker-llm` : chargement GGUF, sessions (1 KV-cache par conversation, éviction LRU bornée), génération streaming annulable, logits → distribution prochains caractères (agrégation BPE→char, gestion des accents/espaces).
- 4.3 Gestion de modèles v0 (D-3.2 minimal) : manifeste JSON + sha256 + reprise de téléchargement + cache local (signatures minisign : phase 7).
- 4.4 `fluence-accel` : assemblage du contexte §5.C (ordre stable→volatil, budget ≤ 2200 tokens, datation relative), prompts v0 `rephrase`/`continue` (tolérance au bruit), post-traitement (dédup, casse, ponctuation), annulation par slot côté hub.
- 4.5 Endpoints réels : `/sessions`, `/turns`, `/draft`, `/suggest` (SSE), `/next-chars` — branchés au superviseur (LLM down → fallback n-gram automatique, événement dégradation).

**Tests**
- T1 : assemblage contexte = **golden prompts** (fixtures lisibles, revue humaine du prompt exact) ; comptage tokens ≤ budget ; datation relative.
- T1/T2 : agrégation BPE→char — propriétés (somme ≈ 1 ; déterminisme ; « bonjou » → « r » dominant avec le tiny-model en greedy).
- T4 : SSE de bout en bout avec tiny-LLM (greedy, seed) : delta → final ; **annulation par slot** : 2e requête sur `main` → la 1re reçoit `aborted` < 50 ms ; kill du worker-llm pendant une génération → `/suggest` bascule n-gram (réponse dégradée signalée, jamais d'erreur 500).
- T6 harnais : `rephrase` (E2B, nightly self-hosted ou local) **bat le n-gram d'au moins +10 points de KS%** sur le corpus v0 — premier critère de valeur ; latences `provisional` tracées par PR (tiny) + contractuelles nightly (E2B).

**Done quand**
- [ ] `fluencectl suggest --mode rephrase "veu eau frache ce soir"` → 3 propositions correctes en français.
- [ ] Rapport harnais : rephrase > n-gram > lettre-à-lettre, publié en nightly.
- [ ] Kill-test LLM : dégradation gracieuse prouvée en CI.

---

### Phase 5 — La boucle complète (sélection + composeur + Piper + urgence) — **L**

**Objectif** : la **première expérience utilisable** : composer au dwell (souris d'abord), voir les suggestions, parler en français. C'est la phase où Fluence devient réel.
**In** : moteur de sélection (cibles/hit-test/dwell fixe + adaptatif), composeur web v0, worker-tts Piper, urgence v0, instrumentation locale. **Out** : regard webcam (phase 6), Tauri (phase 7).

**Tâches**
- 5.1 `fluence-input` v0 : registre de surfaces/cibles (PUT + patchs), hit-testing, **dwell** (progression sur fixation, jauge événementielle, cooldown anti-redéclenchement), source `mouse` (dev/universelle) ; **dwell adaptatif** branché sur `next-chars` (modulation bornée : ±40 % de la durée de base, plancher de sécurité).
- 5.2 `worker-tts` : Piper FR (siwis-medium), streaming Opus/PCM, file P0 prioritaire (préemption des générations LLM testée), fallback voix OS (SAPI / espeak-ng) si worker down.
- 5.3 Composeur web v0 (`apps/web-client`) : layout §7.A (AZERTY adapté, 3 emplacements de suggestions FIXES, draft, PARLER invariant, urgence double confirmation), connexion SDK (WS + SSE), règles anti-scintillement chiffrées (1 maj/600 ms, jamais pendant dwell > 40 %), thème contraste élevé. Servi par le hub (PWA). **i18n-ready dès v0** : toutes les chaînes en clés de traduction (FR seul fourni) — l'architecture language-agnostic (SPEC §1.4) se paie 1 % maintenant ou 20 % plus tard.
- 5.4 Urgence v0 (D-7.4) : cible dédiée → double confirmation → sonnerie locale + bannière sur tous les clients appairés (topic `system`).
- 5.5 Instrumentation locale (P2 data class) : WPM réel, KS% réel, consultations de suggestions — stockée chiffrée, visible par l'utilisateur seul (base de l'étoile polaire).

**Tests**
- T1 input : property tests dwell (« jamais de commit sans fixation cumulée suffisante », « un cooldown suit chaque commit ») ; hit-test aux frontières ; adaptatif borné.
- T4 **replay** : traces de pointeur enregistrées (fixtures LFS) rejouées → séquences de commits attendues au golden (le même replay servira de test de non-régression à chaque refactor du moteur de sélection).
- T4 TTS : premier chunk < 200 ms (provisional CI / contractual nightly) ; préemption : un `speak` pendant une génération LLM part sans attendre ; kill worker-tts → voix OS répond.
- T5 Playwright — scénarios personas : « **Marc compose “bonjour” au dwell-souris et le fait parler** » (assertions : commits corrects, audio émis, draft autosauvé) ; « suggestion acceptée insérée au curseur » ; « urgence : double confirmation obligatoire, bannière reçue par un 2e client appairé » ; « le hub tué pendant la frappe → l'UI se reconnecte, draft intact ».
- T6 : budgets §5.A entrée→décision (< 5 ms hub) et frappe→1er delta suggestion, en provisional.

**Done quand**
- [ ] Démo filmée-reproductible (script) : composer une phrase au dwell, accepter une suggestion, PARLER en Piper FR, déclencher/annuler une urgence.
- [ ] Suite Playwright verte sur les deux OS en CI.
- [ ] Les métriques locales s'affichent (mon premier WPM réel).

---

### Phase 6 — Le regard (webcam + fusion + calibration) — **L/XL**

**Objectif** : le webcam-only utilisable sur cibles moyennes (persona Marc), avec la calibration qui ne demande pas un ingénieur à domicile.
**In** : pipeline regard client web (MediaPipe tasks-vision), mapping few-shot, One Euro + I-VT, fusion tête, calibration initiale/continue/express, tailles de cibles adaptatives. **Out** : modèle de regard maison entraîné (piste ML-regard, démarre ici), trackers IR niveau 1 (OpenGaze — peut glisser post-A1 si nécessaire).

**Tâches**
- 6.1 Client : MediaPipe Face Landmarker → features (iris, pose, géométrie) → publication `PointerSample` au hub (chemin WS §4.A déjà testé). Indicateur de qualité de signal (visage perdu, contre-jour).
- 6.2 Hub : mapping features→écran **few-shot** (régression ridge/polynomiale par profil, format versionné), One Euro (paramètres par profil), I-VT fixation/saccade (le dwell ne progresse que sur fixation — déjà prévu phase 5), fusion « regard désigne, tête affine », magnétisme plafonné 40 % avec priors.
- 6.3 Calibration : initiale **smooth pursuit 45 s** (séquence animée, collecte d'échantillons étiquetés), express 3 points/10 s, **continue implicite** (commits non corrigés < 3 s → paires d'apprentissage, mise à jour lissée), détection de dérive → proposition discrète ; profils de contexte nommés.
- 6.4 Protocole de capture de vérité terrain : outil `fluencectl record-gaze` (cibles affichées + landmarks enregistrés) → datasets internes pour 6.5 et les tests.
- 6.5 **Piste ML-regard (XL, parallèle, continue post-A1)** : modèle ONNX maison type WebEyeTrack entraîné sur les captures — remplace la régression quand il la bat sur le dataset interne.

**Tests**
- T1 : One Euro (réponse à échelon/rampe = valeurs attendues), I-VT (signaux synthétiques → segmentation exacte), magnétisme (jamais > plafond), mise à jour de calibration (lissée, monotone vers la cible).
- T4 replay : sessions de regard enregistrées avec vérité terrain → **% de sélections correctes sur cibles 2,5 cm** calculé en CI ; non-régression stricte (le chiffre ne baisse jamais sans justification).
- T5 : parcours calibration complet simulé par injection (smooth pursuit rejoué) → profil créé, erreur estimée affichée.
- T6 cible : ≥ 95 % de sélections correctes (cibles 2,5 cm, 60 cm) sur nos datasets internes — **critère de pivot** : si < 80 % en fin de phase → on assume des cibles par défaut plus grandes + fusion tête recommandée, et on le documente honnêtement (SPEC Caveats) plutôt que de mentir sur la précision.

**Done quand**
- [ ] Session réelle : calibration 45 s à la webcam puis composer un mot au regard seul (cibles adaptées), démontré sur FLU-REF-4 ou équivalent.
- [ ] Datasets de regard versionnés + chiffre de précision publié en nightly.
- [ ] Qualité de calibration visible en temps réel (base de l'espace aidant).

---

### Phase 7 — Durcissement → **JALON A1** — **L**

**Objectif** : transformer la démo en alpha installable par Sophie en < 30 min — packaging, espace aidant minimal, sécurité des données complète, soak long.
**In** : Tauri desktop (watchdog), espace aidant v0, kit de secours, signatures, soak 72 h, doc. **Out** : tout P2.

**Tâches**
- 7.1 App Tauri : embarque le hub (supervision/watchdog < 2 s, autostart), installeurs signés (MSI/NSIS Windows, AppImage + deb Linux), icônes/branding minimal.
- 7.2 Espace aidant v0 (§7.C réduit) : santé système, qualité de calibration, appareils appairés + révocation, journal d'accès, déclenchement recalibration express.
- 7.3 Données : kit de secours imprimable (QR + phrase, restauration testée), export/restauration chiffrés (**la restauration est un test CI**, pas une promesse), purge totale.
- 7.4 D-3.2 complet : signatures minisign des manifestes, GC des modèles, (pack USB : post-A1 si le temps manque — noté).
- 7.5 Doc : guide d'installation pas-à-pas (Windows/Linux), carte « si ça ne marche plus » (1 page, gros corps), matrice de compatibilité trackers v0 (niveau 0/1 testés).
- 7.6 Self-hosted runner sur machine de référence (au minimum FLU-REF-1 ou équivalent disponible) → bascule des seuils `contractual` ; soak nightly étendu (8 h) + **soak 72 h one-shot** avant le tag.
- 7.7 Passe sécurité interne : checklist threat model §9.A point par point, fuzz corpus à jour, dépendances auditées.

**Tests = les critères A1 de la SPEC (D-12.2), vérifiés mécaniquement**
- [ ] Budgets §5.A tenus en `contractual` sur machine de référence (palier réduit).
- [ ] **KS% ≥ 25 %** (sans contexte conversationnel) sur le corpus d'éval complet.
- [ ] Soak 72 h : zéro crash, RSS borné, zéro perte de draft (kills aléatoires inclus).
- [ ] Installation chronométrée < 30 min par un tiers qui n'a jamais vu le produit (test réel avec une personne de ton entourage, protocole écrit).
- [ ] Suite complète T1–T6 verte sur les deux OS ; couvertures aux gates.
- [ ] Tag `phase-7-done` = **A1 atteint** → revue de SPEC (amendements honnêtes si la réalité a parlé) + re-détaillage du plan P2.

---

## 3. Pistes parallèles longues (démarrent en P1, vivent au-delà)

- **ML-langage (XL)** — démarre après la phase 3 : génération synthétique étendue (corpus v1 ≥ 500 dialogues), distillation **QLoRA par tâche** (teacher grand modèle → student E4B/E2B), éval systématique avant/après sur le harnais (un LoRA n'entre que s'il bat le prompt-only sur le test gelé). Livre ses artefacts aux phases 4+ sans les bloquer.
- **ML-regard (XL)** — démarre en phase 6 (cf. 6.5).
- **Contact upstream (S, continu)** — D-10.3 : ouvrir le dialogue avec AsTeRICS Grid (issue/discussion « external prediction source ») et dasher-web dès la phase 4 (quand on a une API à montrer) ; ça mûrit pendant qu'on construit.

## 4. Esquisse P2/P3 (re-détaillage en fin de phase 7)

Ph8 ASR + replies (bench D-3.4 : Voxtral Realtime vs whisper.cpp vs Gemma 4 audio ; consentement §5.A ; mode replies plein écran) → Ph9 mémoire complète (§5.B : file de validation, ACL, oubli) → Ph10 voix personnelle (§6.A : enregistrement guidé + fine-tuning Piper + F5-TTS différé) → Ph11 abréviations + LoRA en prod → Ph12 intégrations (plugin Grid, dasher-lm, Home Assistant) → Ph13 messageries (email/Matrix d'abord) + appels TTS → Ph14 durcissement → **B1** (KS ≥ 40 %, bêta publique, FluenceBench-FR publié) → P3 : raccordements restants, multilingue, adaptation à la progression, LoRA personnels → **1.0**.

## 5. État d'avancement

| Phase | État | Tag | Notes |
|---|---|---|---|
| 0 — Usine | ✅ terminée (2026-06-13) | `phase-0-done` | 4 workflows verts Win+Linux ; vérifications « l'usine se teste » passées ; détails session 1 |
| 1 — Contrat | ⬜ à lancer | — | prochaine phase |
| 2 — Hub & supervision | ⬜ | — | |
| 3 — Boussole | ⬜ | — | |
| 4 — Moteur | ⬜ | — | |
| 5 — Boucle complète | ⬜ | — | |
| 6 — Regard | ⬜ | — | |
| 7 — Durcissement → A1 | ⬜ | — | |

*Mise à jour de ce tableau à chaque fin de session de travail ; re-détaillage du plan à chaque fin de phase.*

### Journal de session

**Session 1 — 2026-06-13 — Phase 0 complète.**
- **Fait** : repo initialisé et poussé (`Pluenet-Killian/Fluence`, squash-only) ; arborescence §2.B ; SPEC/PLAN/Project versés dans `docs/` ; gouvernance (CONTRIBUTING, SECURITY D-9.3, CODE_OF_CONDUCT) ; templates PR/issues (dont `debt`) ; licences D-10.1 (textes canoniques, copies par sous-arbre, en-têtes SPDX vérifiés par `cargo xtask check-licenses` — testé sur le repo par un test d'intégration) ; `deny.toml` ; 3 workspaces (Cargo resolver 3 / edition 2024 / MSRV 1.89 / clippy pedantic / forbid unsafe ; pnpm TS 6 strict + eslint 10 flat strictTypeChecked + vitest 4 + prettier ; uv ml/ à racine virtuelle + ruff + mypy strict) ; 7 crates + xtask + 3 packages TS + 3 packages Python avec un test réel chacun (invariant de licence D-10.1) ; hooks lefthook < 5 s (mesurés 0,8–2,7 s) + commitlint ; 4 workflows CI ; ADR-0001/0002/0003.
- **Vérifications « l'usine se teste »** : builds+tests verts localement sur les 3 écosystèmes ; CI verte Windows+Linux sur main (ci, integration, nightly, release — tous exécutés) ; PR-cobaye lint cassé → `ci` rouge (PR #1) ; PR-cobaye commit non conventionnel → hook local rouge (vérifié) + job commitlint rouge (PR #2) ; dépendance MPL-2.0 ajoutée → `cargo deny` rouge (exit 4), retirée.
- **Déviations consignées** : `ml/` et `xtask/` sous Apache-2.0 (non couverts par D-10.1 — justification ADR-0003 §5) ; `apps/*` en placeholders README jusqu'à leurs phases (ADR-0003 §10, la Phase 0 exclut le code fonctionnel) ; frontière de langue code EN / docs FR (ADR-0003 §9).
- **Environnement local (pas une dette projet)** : un driver anti-cheat kernel (Riot Vanguard) bloque la création des lanceurs `.exe` d'uv sur la machine de dev — contournement documenté dans CONTRIBUTING (« python -m », venv seedé pip) ; la CI n'est pas affectée. À réévaluer si uv ajoute une option sans ressources PE.
- **Dette** : formatteur YAML/TOML absent → issue `debt` #3.
- **Reprise session suivante** : Phase 1 (« Le contrat ») — commencer par 1.1 (types FluenceInput v1 dans `fluence-protocol` avec schemars), la chaîne `xtask check-contracts` (1.3) lève son stub `exit 2`.
