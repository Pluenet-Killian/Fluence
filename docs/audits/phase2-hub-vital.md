Je vais rédiger le rapport final directement. J'ai toutes les données vérifiées nécessaires.

## 1. Verdict Adversarial

**Note de crash-test : 6,5 / 10.**

Le hub Phase 2 tient ses invariants vitaux sous les scénarios nominaux : le chemin clavier est décorrélé de l'IA, le défaut d'écoute est loopback, le store est chiffré et le pairing est correctement borné par fenêtre. Aucune des accusations « critical » d'origine ne survit à la vérification — pas d'IDOR exploitable hors sur-octroi délibéré, pas d'élévation de privilège (la route /pair/window est system-only et le mode foyer LAN n'est pas codé), pas de perte de données sur la voie courante de frappe. Le projet perd néanmoins des points sur une **absence systématique de garde-fous de robustesse** : tout ce qui sort du régime mono-utilisateur idéal (panne disque, flood local, course delete/flush, croissance non bornée) est non borné, non testé, ou se dégrade silencieusement. C'est un code « heureux-chemin » solide, mais sans ceinture de sécurité.

Les pires faiblesses :

- **Aucune borne, nulle part.** dirty_drafts (RAM), la table drafts (disque), le journal d'accès (disque + fsync), les connexions WS, la durée d'un flush : cinq surfaces de croissance pilotées par l'input d'un device local, toutes sans plafond ni TTL ni rate-limit (F09, F26, F15, F20, G7). Un seul device Control bogué ou malveillant — déjà dans la frontière de confiance — peut les saturer.
- **La durabilité P0 a deux trous structurels confirmés.** flush_drafts draine la map AVANT l'upsert (perte sur erreur IO store persistante, F01) et la course delete_session/flusher peut RESSUSCITER un draft P0 d'une conversation fermée dans le store chiffré (F10). Pour un projet où « aucune donnée P0 contre la volonté de l'utilisateur » est sacré, c'est gênant.
- **Le chiffrement at-rest est annulable par configuration, en silence.** En mode File (défaut headless Linux), la clé maître en clair atterrit à côté de store.db, sans aucun `tracing::warn!` au runtime (F06). La couverture « voleur du PC » de la SPEC tombe pour ces installs.
- **Diagnostic et cycle de vie bâclés.** Une erreur d'écriture de token déguisée en erreur de bind réseau (F30), un flusher détaché jamais annulé (F17), un shutdown qui n'attend pas les workers (F16), un device System orphelin re-minté à chaque boot raté sans révocation (G2) : dette de fiabilité qui mordra en Phase 4+.
- **Aucun de ces trous n'a de test rouge.** Le projet impose pourtant « bug = test rouge d'abord ». Les kill-tests existants couvrent kill -9 / RSS mais aucun ne couvre le flood, la course, la saturation disque, ni la perte sur panne IO.

---

## 2. Registre des Risques Majeurs

### [MEDIUM] F09 — dirty_drafts non bornée + session_id non validé → épuisement disque
**Fichier :** `crates/fluence-hub/src/state.rs:138-142`

**Ce qui casse.** `buffer_draft` insère par `session_id` sans plafond de cardinalité ni éviction ; `put_draft` accepte n'importe quel `session_id` de path sans vérifier existence ni appartenance. La table `drafts` (PRIMARY KEY `session_id`, aucun TTL, aucune FK) croît sans borne. Un device Control bouclant des PUT sous des UUID distincts remplit le disque du foyer.

**Pourquoi (cause racine).** Collection persistante non bornée pilotée par l'input client ; aucune politique de rétention côté store, aucune liaison session→device.

**Déclencheur concret.** Token Control valide : `for i in 0..1_000_000 { PUT /api/v1/sessions/<uuid_i>/draft }` sans `create_session`. Chaque appel écrit une ligne ; la table grossit jusqu'à saturation sur la cible 8 Go (Marc).

**Citation.**
```rust
pub fn buffer_draft(&self, session_id: String, draft: PendingDraft) {
    lock(&self.0.dirty_drafts).insert(session_id, draft);
}
```
*Rectificatif vérifié :* l'OOM RAM décrit à l'origine est faux — le flusher draine toute la map toutes les 500 ms et axum plafonne chaque corps à 2 Mo. Le résidu réel est l'épuisement **disque**, lent et persistant.

### [MEDIUM] F01 — flush_drafts draine avant l'upsert → perte de draft sur erreur IO persistante
**Fichier :** `crates/fluence-hub/src/state.rs:158-179`

**Ce qui casse.** `take_dirty_drafts()` vide inconditionnellement la HashMap, puis chaque `upsert_draft` est tenté ; sur `Err`, le code ne fait qu'un `tracing::error!` sans re-bufferiser. Le draft quitte la RAM sans être persisté.

**Pourquoi (cause racine).** Le drain-puis-upsert n'est pas atomique : l'invariant « le draft reste bufferisé tant qu'il n'est pas confirmé persisté » n'est pas tenu.

**Déclencheur concret.** Saturer le FS pendant un tick de flush : le draft drainé est loggé puis jeté. Si l'utilisateur a cessé de taper (pas de re-buffer par frappe suivante) et que le store reste en erreur jusqu'au crash, le draft est perdu.
*Rectificatif vérifié :* le SQLITE_BUSY par verrou concurrent et le `Closed` « sous charge » sont architecturalement impossibles (store mono-connexion sérialisée). Ce n'est PAS une violation de la garantie power-loss de D-2.6 ; seul subsiste le cas erreur IO réellement persistante (disque plein) + arrêt de frappe + crash.

**Citation.**
```rust
pub async fn flush_drafts(&self) {
    for (session_id, draft) in self.take_dirty_drafts() {
        if let Err(error) = self.store().upsert_draft(...).await {
            tracing::error!(%error, "draft flush failed");
        }
    }
} // take_dirty_drafts => lock(&self.0.dirty_drafts).drain().collect()
```

### [MEDIUM] F20 — flush sériel de N drafts (fsync FULL un par un) → durée non bornée
**Fichier :** `crates/fluence-hub/src/state.rs:158-179`

**Ce qui casse.** `flush_drafts` persiste chaque draft séquentiellement, chacun via un commit `synchronous=FULL` (1 fsync). Avec beaucoup de sessions dirty, un tick peut dépasser 500 ms voire 1 s ; un kill -9 pendant ce temps perd >1 s de frappe pour la session légitime.

**Pourquoi (cause racine).** Boucle strictement séquentielle (`for ... { upsert_draft(...).await }`), store mono-thread, `MissedTickBehavior::Delay` qui retarde le tick suivant de la durée du flush. Coût = N × (fsync + queue).

**Déclencheur concret.** Bufferiser 5000 sessions distinctes, taper dans une session légitime, puis kill -9 pendant que le tick traite les 5000 upserts fsync.

**Citation.**
```rust
for (session_id, draft) in self.take_dirty_drafts() {
    if let Err(error) = self.store().upsert_draft(...).await { ... }
} // N upserts fsync FULL séquentiels, le tick suivant attend la fin
```
*Rectificatif vérifié :* en usage mono-utilisateur réel (N minuscule) ne se déclenche jamais ; déclencheur = auto-DoS / client local bogué, pas une menace externe.

### [MEDIUM] F26 — journal d'accès non borné, amplifié par auth.rejected non authentifié
**Fichier :** `crates/fluence-store/src/actor.rs:320-331`

**Ce qui casse.** Chaque échec d'auth (`auth.rs:87`, `ws.rs:84`) écrit une ligne `access_journal` via un INSERT pur, sans rétention ni rate-limit. Le vrai défaut n'est pas tant la saturation disque (millions de requêtes nécessaires) que la **contention IO** : chaque INSERT est un fsync FULL sur la connexion unique partagée avec le draft-flusher.

**Pourquoi (cause racine).** `journal_append` ne fait qu'un INSERT ; aucune purge/rotation ; les chemins d'échec journalisent inconditionnellement sans authentification ni throttle.

**Déclencheur concret.** `for _ in ..; do GET /ws?...&token=flt_bidon; done` ou `GET /system/health` avec `X-Fluence-Token` bidon. Chaque appel = 1 INSERT + 1 fsync sur le thread store, retardant les flush de drafts (menace D-2.6) bien avant toute saturation disque.

**Citation.**
```rust
fn journal_append(conn, entry) {
    conn.execute("INSERT INTO access_journal (at, device_id, action, detail) VALUES (?1, ?2, ?3, ?4)", ...)
} // aucune rétention ; appelé inconditionnellement sur chaque auth.rejected
```

### [MEDIUM] F06 — clé maître en clair à côté de store.db (mode File) → at-rest neutralisé
**Fichier :** `crates/fluence-hub/src/lib.rs:94-104`

**Ce qui casse.** En mode File, `KeySource::File` écrit la clé SQLCipher (64 hex) en clair dans `data_dir`, à côté de `store.db`. Copier le dossier livre base + clé. Aucun `tracing::warn!` runtime n'avertit que ce mode dégrade la couverture « voleur du PC » (SPEC §9.A).

**Pourquoi (cause racine).** `hex::encode` sans wrapping/KDF/passphrase, fichier co-localisé par convention ; manque uniquement un garde-fou runtime et une note doc tranchante.

**Déclencheur concret.** `FLUENCE_STORE_KEY_FILE=$DATA/store.key`, puis exfiltrer le dossier : `sqlcipher store.db "PRAGMA key=\"x'$(cat store.key)'\"; SELECT text FROM drafts;"`.

**Citation.**
```rust
key: match &config.store_key_file {
    Some(path) => KeySource::File(path.clone()),
    None => KeySource::Keyring { ... }
}  +  StoreConfig{ path: config.data_dir.join("store.db"), ... }
```
*Rectificatif vérifié :* le défaut de prod est le keystore OS (DPAPI/Secret Service), File est opt-in documenté comme repli ; le finding surestimait en disant « recommandé » et « rien n'avertit ».

### [MEDIUM] F08 — FLUENCE_LISTEN_ADDR non-loopback sans TLS → tokens et P0 sniffables
**Fichier :** `crates/fluence-hub/src/config.rs:124-130`

**Ce qui casse.** `apply_env` accepte n'importe quelle IP sans contrainte loopback, `bind_with_fallback` la bind telle quelle, et aucune couche TLS n'existe (`ca_fingerprint` reste `None`). `FLUENCE_LISTEN_ADDR=0.0.0.0` met le hub en HTTP clair sur le LAN : pairing codes, device tokens, texte P0 des drafts circulent en clair.

**Pourquoi (cause racine).** Le choix de l'adresse est dissocié de la disponibilité de TLS ; rien ne refuse un bind non-loopback tant que TLS est absent.

**Déclencheur concret.** `FLUENCE_LISTEN_ADDR=0.0.0.0 fluence-hub` puis sniff Wireshark sur `GET /api/v1/sessions/{id}/draft` depuis une autre machine du LAN.

**Citation.**
```rust
if let Some(value) = lookup("FLUENCE_LISTEN_ADDR") {
    self.listen_addr = value.parse().map_err(...)?;
} // aucune contrainte loopback, aucun lien avec TLS
```
*Rectificatif vérifié :* défaut loopback (`config.rs:45`), Phase 2 en cours, le mode foyer LAN+TLS (tâche 2.5 / « PR C ») n'est pas encore livré. Item prospectif, pas une régression active.

### [MEDIUM] F10 — course delete_session vs flusher → draft P0 d'une session supprimée ressuscité
**Fichier :** `crates/fluence-hub/src/api/sessions.rs:27-39`

**Ce qui casse.** Le texte P0 d'une conversation explicitement DELETE peut être réinséré dans le store chiffré et y survivre, contre la volonté de l'utilisateur (SPEC §9.A).

**Pourquoi (cause racine).** `discard_pending_draft(id)` puis `delete_draft(id).await` s'exécutent concurremment au flusher. Si le flusher draine S (rendant `discard` no-op) puis que `delete_draft` précède `upsert` dans le canal FIFO du store, l'INSERT du flusher arrive après le DELETE et réinsère le draft. L'ordre relatif des deux `tx.send()` depuis deux tâches n'est pas garanti.

**Déclencheur concret.** Taper dans S, puis `DELETE /api/v1/sessions/S` pile pendant un tick du flusher. Fenêtre sub-milliseconde mais atteignable sur runtime multi-thread sans point de yield requis.

**Citation.**
```rust
state.discard_pending_draft(&session_id);
match state.store().delete_draft(session_id).await { ... }
// concurrent avec flush_drafts -> upsert_draft du même session_id, même canal, ordre non déterminé
```

### [MEDIUM] F15 — aucune limite de connexions WS simultanées ni quota par device
**Fichier :** `crates/fluence-hub/src/api/ws.rs:60-101`

**Ce qui casse.** Un seul device avec un token Display (le plus faible, suffit pour /ws) peut ouvrir des milliers de connexions /ws. Chaque `on_upgrade` lance une tâche `serve()` + un `broadcast::Receiver` + 1 FD. Au plafond FD OS (1024 par défaut), le clavier devient indisponible.

**Pourquoi (cause racine).** `build_router` monte /ws sans `ConcurrencyLimit` ni quota ; `axum::serve` n'impose aucun plafond ; `AppState` ne compte pas les WS par device.

**Déclencheur concret.** Token Display valide : `for i in 0..10000 { connect("ws://127.0.0.1:7411/ws?...&token=<t>") }` en gardant les sockets ouverts.

**Citation.**
```rust
let websocket = Router::new().route("/ws", get(ws::upgrade));
... .merge(websocket).layer(tower_http::cors::CorsLayer::new())
// aucune couche de limite de concurrence/connexions
```
*Rectificatif vérifié :* déclencheur = device déjà appairé (pairing manuel requis), loopback par défaut. Garde-fou de robustesse pour le mode maison opt-in.

### [MEDIUM] G7 — pas de DefaultBodyLimit explicite + drafts ~2 Mo × N sessions
**Fichier :** `crates/fluence-hub/src/api/mod.rs:139-147`

**Ce qui casse.** Le routeur n'impose aucun `DefaultBodyLimit` explicite ; on dépend du défaut implicite d'axum (~2 Mo), non documenté et susceptible de changer entre versions. Combiné à `buffer_draft` sans plafond, une rafale de corps proches de 2 Mo sur des session_id distincts crée une pression RAM.

**Pourquoi (cause racine).** Limite de corps non réglée explicitement ; aucun plafond de taille cumulée sur dirty_drafts.

**Déclencheur concret.** Token Control : boucle de `PUT` avec `text: <~2 Mo>` sur des UUID distincts.

**Citation.**
```rust
Router::new().merge(public).merge(authed).merge(websocket)
    .layer(tower_http::cors::CorsLayer::new()).with_state(state)
// aucun .layer(DefaultBodyLimit::...)
```
*Rectificatif vérifié :* le flusher draine toute la map toutes les 500 ms, donc pas de croissance RAM monotone ; l'exposition durable se reporte sur le disque (recoupe F09). Le vrai durcissement manquant est la limite de corps **explicite**.

### [MEDIUM] G2 — device System inséré AVANT l'écriture du token → orphelins re-mintés sans révocation
**Fichier :** `crates/fluence-hub/src/lib.rs:189-204`

**Ce qui casse.** Si `std::fs::write(system.token)` échoue après l'`insert_device`, le device System est persisté mais son token plaintext est perdu. Au reboot, le fichier absent fait re-minter un NOUVEAU device System sans révoquer l'ancien → accumulation de lignes System orphelines à chaque boot raté.

**Pourquoi (cause racine).** L'effet durable (insert store) précède l'effet faillible (écriture fichier), sans transaction ni compensation ; `create_dir_all` appelé APRÈS l'insert.

**Déclencheur concret.** Premier boot sur volume plein/lecture seule : insert réussit, `fs::write` échoue → `HubError::Bind`, device orphelin persisté. Libérer l'espace, reboot → 2e device System.

**Citation.**
```rust
state.store().insert_device(... scope: Scope::System ...).await?;
std::fs::create_dir_all(&state.config().data_dir).ok();
std::fs::write(&path, &token).map_err(|source| HubError::Bind {
    addr: SocketAddr::new(state.config().listen_addr, 0), source,
})?;
```
*Rectificatif vérifié :* la ligne orpheline est INERTE (le store ne garde qu'un SHA-256, aucun plaintext correspondant n'existe) — pas un credential omnipotent exploitable, et le hub refuse de démarrer (échec explicite, auto-guérison au boot réussi). Le résidu réel est l'hygiène (accumulation) + l'atomicité.

### [LOW] Findings confirmés mais de sévérité finale faible

| id | titre (résumé) | fichier:lignes | essentiel |
|----|----------------|----------------|-----------|
| F02 | IDOR drafts inter-Control | `api/sessions.rs:19-85` | Réel mais le scope Care (aidant) est déjà à 403 ; UUID v4 non énumérable ; exige sur-octroi délibéré de Control. Défense-en-profondeur. |
| F03 | /pair/window accepte scope:system | `api/pair.rs:21-51` | Réel mais non-escalade : route system-only, exige déjà le token System omnipotent ; LAN non codé. Durcissement avant PR C. |
| F05 | TOCTOU clé Unix + pas d'ACL Windows | `fluence-store/src/key.rs:82-98` | TOCTOU Unix réel mais étroit ; le défaut Windows de prod = keystore OS, aucun fichier clé en clair. |
| F07 | system.token clair, sans TTL, chmod best-effort | `fluence-hub/src/lib.rs:159-199` | Dans l'exclusion documentée du threat model (process même-user). Seul vrai défaut : chmod avalé (incohérent avec key.rs). |
| F11 | store illisible → hub ne démarre pas | `fluence-store/src/actor.rs:137-164` | « clavier toujours » vise les composants IA ; résilience hub-down = côté client (SPEC §2.C). Échouer fort est correct. |
| F13 | canal store unique → latence d'auth | `fluence-hub/src/auth.rs:85-90` | Erreur factuelle : l'ACK clavier ne passe PAS par le canal store (buffer en RAM). Latence clavier décrite = fausse. |
| F14 | WS half-open jamais détectée | `fluence-hub/src/api/ws.rs:129-174` | Fuite BORNÉE par le retransmit-timeout TCP (minutes), pas illimitée ; loopback par défaut. |
| F16 | shutdown n'attend pas les workers | `fluence-hub/src/lib.rs:65-77` | `kill_on_drop(true)` envoie le SIGKILL synchrone au teardown ; pas d'orphelin vivant démontré. |
| F17 | flusher périodique jamais annulé | `fluence-hub/src/state.rs:193-204` | Réfuté : axum draine les requêtes avant flush_drafts (map gelée), Close est FIFO-ordonné. Propreté, pas perte. |
| F18 | lag broadcast perd 'down' | `fluence-hub/src/api/ws.rs:131-149` | /system/health est l'état de record ; sous flapping le ring retient Starting/Down, jamais un Ready périmé. |
| F19 | reset backoff sur flapping | `fluence-hub/src/supervisor/mod.rs:147-192` | Plancher 150-250 ms (jamais zéro), clavier découplé ; kill-test RSS borne à +10 %. |
| F23 | code pairing non constant-time | `fluence-hub/src/api/pair.rs:95-103` | Signal ~7 ns noyé sous l'écriture journal awaitée ; lockout 5 essais + code frais empêchent l'agrégation. Infaisable. |
| F27 | fuite fichiers socket Unix | `fluence-hub/src/supervisor/mod.rs:209-213` | Réel (Unix only) mais débit borné (cap backoff 10 s), fichiers 0 octet ; le « bind impossible » décrit est faux (chemins uniques). |
| F29 | une ligne timestamp corrompue casse list_devices | `fluence-store/src/actor.rs:333-349` | Timestamps écrits uniquement par `Utc::now().to_rfc3339()` (round-trip garanti) ; révocation par id indépendante. Déclencheur irréaliste. |
| F30 | erreur I/O token déguisée en Bind(port 0) | `fluence-hub/src/lib.rs:188-192` | Diagnostic trompeur réel ; déclencheur courant intercepté plus tôt par Store::open. Pas de fuite/perte. |
| F31 | erreurs avalées (let _ / .ok()) | `fluence-hub/src/lib.rs:66-77` | 2 sous-claims faux positifs (create_dir_all redondant, served.await bénin) ; seul réel = chmod token avalé, étroit. |
| F32 | AppState god-object | `fluence-hub/src/state.rs:67-205` | Sur-estimé : sous-services déjà découpés (EventBus, supervisor, Store) ; AppState = conteneur d'état (pattern axum). |
| F35 | expiration pairing dupliquée, pas de purge | `fluence-hub/src/api/pair.rs:54-68` | Code résiduel fonctionnellement MORT (pair_device vérifie l'expiration avant comparaison). Dette + hygiène mémoire cosmétique. |
| F36 | auth dupliquée header vs ws | `fluence-hub/src/api/ws.rs:76-92` | La décision de sécurité (lookup + révocation) est centralisée dans le store ; seule la coquille réponse/journal est dupliquée. |
| G1 | fenêtre pairing consommée avant commit | `fluence-hub/src/api/pair.rs:104-170` | Déclencheurs faux (send().await = backpressure, pas SQLITE_BUSY interne). Résidu = erreur store persistante / collision UUID. |
| G3 | hub.port jamais supprimé ni atomique | `fluence-hub/src/lib.rs:126-129` | Aucun client de prod ne lit hub.port (fluencectl/desktop = stubs, SDK utilise baseUrl explicite). Durcissement Phase 2. |
| G4 | put_draft ne borne pas caret | `fluence-hub/src/api/sessions.rs:45-60` | Aucun consommateur n'indexe le texte par caret en Phase 2 ; crash futur dépend du code Phase 5. Clamp trivial. |
| G6 | detect_tier reconstruit sysinfo par requête | `fluence-hub/src/api/system.rs:41-60` | **Sévérité finale : none.** Coût = 1-2 syscalls, écrasé par le lookup SQLCipher d'auth. Optimisation, pas DoS. |

### Écartés après vérification

- **F04** (busy-loop IPC + heartbeat neutralisé) — le code sur disque fait l'inverse exact (`Ok(None) | Err(_) => kill + Died`) ; finding généré contre un snapshot antérieur, fix déjà appliqué.
- **F12** (put_draft sans vérif d'existence de session) — le design n'a aucune notion de propriété session→device (choix produit, SPEC §2.A) ; aucun contrôle d'existence n'apporterait de sécurité.
- **F21** (récupération mutex empoisonnés masque corruption) — aucune section critique ne peut paniquer (pas d'`.await` sous guard, pas de Hash custom) ; code défensif mort.
- **F22** (révocation ne purge pas les drafts) — comportement intentionnel et documenté ; protection « appareil volé » = chiffrement (SPEC §9.A), pas révocation. Entièrement parasite de F02.
- **F24** (token /ws en query leakable) — aucun `query.token` n'atteint un macro `tracing!`, aucun `TraceLayer` ; révocation existe (donc pas « sans TTL irrémédiable »).
- **F25** (pas de zeroization P0) — zeroization RAM systématique explicitement **hors scope** (SPEC §9.A:513) ; déclencheur (core dump, /proc/mem) dans les acteurs non couverts.
- **F28** (horloge avant epoch → updated_at_micros=0) — le champ n'est jamais relu pour une décision ; le kill-test mesure par égalité de texte, pas par timestamp.
- **F33** (version /ws vérifiée avant auth) — la version de protocole est publique par conception (OpenAPI, /pair/info anonyme) ; aucune valeur de fingerprinting.
- **F34** (réouverture pairing reset lockout) — chaque réouverture tire un code aléatoire neuf ; loteries indépendantes, l'opposé d'une faiblesse de brute-force.
- **F37** (famine d'arrêt select! sans biased) — repose sur F04 (réfuté) ; `select!` repoll les branches prêtes, délai espéré ~2 itérations, pas une famine.
- **F38** (test anti-fuite P0 lit stdout après kill -9) — le fmt layer écrit synchrone + `\n` final → LineWriter pousse au noyau immédiatement ; survit au kill -9 (vérifié empiriquement).
- **G5** (open_window ne purge pas l'ancien code) — écrasement = comportement spécifié (« une fenêtre à la fois ») ; code pairing destiné à être affiché, non-P0 ; appelant = UI System de confiance.

---

## 3. Plan de Remédiation Radical

### Cluster A — Durabilité & cohérence P0 du buffer de drafts (F01, F10, F20)

Cause racine commune : `flush_drafts` traite le buffer de manière non atomique et non coordonnée avec les suppressions, et persiste séquentiellement. Les trois patches touchent `state.rs`/`actor.rs` et doivent être intégrés ensemble (le patch F20 reprend le modèle tombstone/génération introduit par F10).

#### F01 — flush sans perte sur erreur IO transitoire
**Approche.** Remplacer le drain-puis-upsert par un cycle clone-puis-remove-conditionnel : flusher une COPIE du buffer (lock relâché avant tout await), et ne retirer une entrée QUE si son upsert a réussi ET que l'entrée présente porte toujours le même `updated_at_micros`. Le draft ne quitte la RAM qu'une fois la persistance confirmée.

```rust
// ============================================================================
// crates/fluence-hub/src/state.rs
// ============================================================================

// --- 1. PendingDraft: a cheap, explicit snapshot helper (P0 stays behind
//        SecretString; no Clone derive so the secret is never copied
//        implicitly). ---

impl PendingDraft {
    /// Clones the draft for an out-of-lock flush attempt. The P0 text stays
    /// wrapped in `SecretString`; cloning is explicit so a copy never escapes
    /// silently.
    fn snapshot(&self) -> Self {
        Self {
            text: self.text.clone(),
            caret: self.caret,
            updated_at_micros: self.updated_at_micros,
        }
    }
}

// --- 2. Replace `take_dirty_drafts` with a non-destructive snapshot + a
//        confirmed-only removal. The buffer is now the source of truth until
//        the store *confirms* the write. ---

impl AppState {
    /// Snapshots every dirty draft for a flush attempt **without** emptying
    /// the buffer: a draft only leaves RAM once the store confirms its write
    /// (see [`AppState::clear_flushed_draft`]). This is what keeps the
    /// «&nbsp;buffered until persisted&nbsp;» invariant — and therefore the
    /// D-2.6 loss bound — true across a transient store failure (disk full,
    /// WAL/FS error, closed store).
    fn snapshot_dirty_drafts(&self) -> Vec<(String, PendingDraft)> {
        lock(&self.0.dirty_drafts)
            .iter()
            .map(|(session_id, draft)| (session_id.clone(), draft.snapshot()))
            .collect()
    }

    /// Removes a draft from the buffer **only** if it is still the exact
    /// version we just persisted (`updated_at_micros` witness). If the user
    /// typed again, or the session was closed
    /// ([`AppState::discard_pending_draft`]), the buffer holds a newer state
    /// (or nothing) which must survive — so we leave it for the next tick.
    fn clear_flushed_draft(&self, session_id: &str, flushed_at_micros: u64) {
        let mut drafts = lock(&self.0.dirty_drafts);
        if drafts
            .get(session_id)
            .is_some_and(|current| current.updated_at_micros == flushed_at_micros)
        {
            drafts.remove(session_id);
        }
    }

    /// Flushes all dirty drafts to the store. Called by the periodic flusher
    /// and by graceful shutdown.
    ///
    /// Durability contract (D-2.6): a draft is removed from the buffer
    /// **only after** the store acknowledges the write. On a transient store
    /// error the draft stays buffered and is retried on the next tick
    /// (≤ `DRAFT_FLUSH_PERIOD` later), so an acknowledged keystroke is never
    /// lost from both RAM and disk. The error is logged **without** the P0
    /// text (`StoreError`'s `Display` never carries draft content).
    pub async fn flush_drafts(&self) {
        for (session_id, draft) in self.snapshot_dirty_drafts() {
            let flushed_at_micros = draft.updated_at_micros;
            match self
                .store()
                .upsert_draft(
                    session_id.clone(),
                    draft.text,
                    draft.caret,
                    flushed_at_micros,
                )
                .await
            {
                // Persisted: drop it from the buffer, but only if no fresher
                // keystroke arrived meanwhile (else we'd lose the newer text).
                Ok(()) => self.clear_flushed_draft(&session_id, flushed_at_micros),
                // Transient store failure: keep the draft buffered for retry.
                // Logging the session id is fine (it is a UUID, not P0); the
                // text is never logged.
                Err(error) => {
                    tracing::error!(%error, %session_id, "draft flush failed; kept buffered for retry");
                }
            }
        }
    }
}

// NOTE: `take_dirty_drafts` is removed. It was only used by `flush_drafts`
// (confirmed via grep) and its drain-then-upsert was the non-atomic step that
// dropped a P0 draft on store error. If an external caller still needs a
// destructive take, reintroduce it as a distinct method — do NOT route the
// flush path through it.
```

**Notes.** Tests rouge→vert (T2/T3) : Store fermé/mocké → `buffer_draft` → `flush_drafts` (erreur) → `pending_draft` renvoie toujours le draft ; puis store sain → flush → buffer vide. Test de fraîcheur : v1 → flush échec → v2 → flush OK → store contient v2. Aucun `std::sync::Mutex` tenu à travers un await (clippy `await_holding_lock`). Perf : un clone SecretString par draft par tick (≤2 writes/s), négligeable vs le fsync déjà payé. Le `%session_id` (UUID, non-P0) ajouté ne doit pas casser `hub_logs_never_contain_draft_content`.

#### F10 — sérialiser delete-vs-flush via tombstone + génération
**Approche.** Chaque draft bufferisé porte une génération monotone (`AtomicU64`) ; `delete_session` pose un tombstone `deleted_at[session]=generation` sous le même mutex ; le flusher re-vérifie le tombstone sous ce mutex juste avant l'upsert et supprime l'écriture si `tombstone >= generation`. Le DELETE devient autoritaire : aucun `UpsertDraft` n'est jamais envoyé pour un draft que le delete a observé, donc l'ordre du canal FIFO ne joue plus.

```rust
// ===== crates/fluence-hub/src/state.rs =====

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Buffered draft plus the autosave generation it was written at. The
/// generation is the linchpin that closes the delete-vs-flush race
/// (F10): the flusher captures it on drain and re-checks it just before
/// persisting, so a draft deleted (or re-typed) meanwhile can never be
/// resurrected in the encrypted store.
struct BufferedDraft {
    draft: PendingDraft,
    /// Strictly increasing tag assigned at `buffer_draft` time.
    generation: u64,
}

/// What the flusher carries out of a drain tick: the draft to persist and
/// the generation it was buffered at.
struct DrainedDraft {
    session_id: String,
    draft: PendingDraft,
    generation: u64,
}

/// Coordination state shared between writers (HTTP handlers) and the
/// flusher, guarded by one mutex so every decision is serialized.
///
/// `dirty` holds the latest unflushed draft per session; `deleted_at`
/// records, per session, the generation current when the session was last
/// closed. A flush of generation `g` is suppressed whenever the session
/// has a tombstone `t >= g` — a delete that observed that draft (or a
/// newer one) must win, never the in-flight upsert.
struct DraftBuffer {
    dirty: HashMap<String, BufferedDraft>,
    deleted_at: HashMap<String, u64>,
}

struct Inner {
    config: HubConfig,
    store: Store,
    bus: EventBus,
    started_at: DateTime<Utc>,
    pairing: Mutex<Option<PairingWindow>>,
    drafts: Mutex<DraftBuffer>,
    /// Monotonic source for draft generations (never wraps in practice:
    /// <= 2 writes/s for centuries).
    draft_generation: AtomicU64,
    workers: Mutex<Vec<Arc<WorkerHandle>>>,
}

// in AppState::new(...)
//   drafts: Mutex::new(DraftBuffer { dirty: HashMap::new(), deleted_at: HashMap::new() }),
//   draft_generation: AtomicU64::new(0),

impl AppState {
    /// Buffers a draft write (overwrites a previous unflushed one — only
    /// the latest state matters). A fresh write also clears any tombstone:
    /// typing again into a previously closed session reopens it.
    pub fn buffer_draft(&self, session_id: String, draft: PendingDraft) {
        let generation = self.0.draft_generation.fetch_add(1, Ordering::Relaxed);
        let mut drafts = lock(&self.0.drafts);
        drafts.deleted_at.remove(&session_id);
        drafts
            .dirty
            .insert(session_id, BufferedDraft { draft, generation });
    }

    #[must_use]
    pub fn pending_draft(&self, session_id: &str) -> Option<PendingDraft> {
        lock(&self.0.drafts)
            .dirty
            .get(session_id)
            .map(|b| PendingDraft {
                text: b.draft.text.clone(),
                caret: b.draft.caret,
                updated_at_micros: b.draft.updated_at_micros,
            })
    }

    /// Drains every dirty draft for persistence, tagged with its generation.
    /// Also prunes tombstones once `dirty` is empty: nothing buffered means
    /// no upsert can still be in flight, so `deleted_at` stays bounded by
    /// the live working set, not by history. (Note: only ever called from
    /// `flush_drafts`, which runs serially.)
    fn take_dirty_drafts(&self) -> Vec<DrainedDraft> {
        let mut drafts = lock(&self.0.drafts);
        let drained: Vec<DrainedDraft> = drafts
            .dirty
            .drain()
            .map(|(session_id, buffered)| DrainedDraft {
                session_id,
                draft: buffered.draft,
                generation: buffered.generation,
            })
            .collect();
        drafts.deleted_at.clear();
        drained
    }

    /// Discards the unflushed draft of a closing session and plants a
    /// tombstone so a concurrently draining flusher cannot resurrect it.
    /// Makes a `DELETE` authoritative against the autosave loop — a closed
    /// conversation's P0 never outlives it (SPEC §9.A; closes F10).
    pub fn discard_pending_draft(&self, session_id: &str) {
        let generation = self.0.draft_generation.fetch_add(1, Ordering::Relaxed);
        let mut drafts = lock(&self.0.drafts);
        drafts.dirty.remove(session_id);
        // Keep the highest generation if a previous (unflushed) tombstone
        // exists — never weaken an existing delete decision.
        let slot = drafts.deleted_at.entry(session_id.to_owned()).or_insert(0);
        *slot = (*slot).max(generation);
    }

    /// Flushes all dirty drafts to the store. The upsert is suppressed when
    /// the session was closed since it was buffered (`tombstone >=
    /// generation`): the `DELETE` wins, so a freshly closed conversation is
    /// never written back (F10 / SPEC §9.A). The check is under the buffer
    /// lock, just before the async store call; the lock spans only a
    /// `HashMap` lookup, never an `.await`.
    pub async fn flush_drafts(&self) {
        for drained in self.take_dirty_drafts() {
            let suppressed = {
                let drafts = lock(&self.0.drafts);
                drafts
                    .deleted_at
                    .get(&drained.session_id)
                    .is_some_and(|&tombstone| tombstone >= drained.generation)
            };
            if suppressed {
                continue;
            }
            if let Err(error) = self
                .store()
                .upsert_draft(
                    drained.session_id,
                    drained.draft.text,
                    drained.draft.caret,
                    drained.draft.updated_at_micros,
                )
                .await
            {
                tracing::error!(%error, "draft flush failed");
            }
        }
    }
}

// ===== crates/fluence-hub/src/api/sessions.rs (handler inchange dans son corps) =====
// delete_session continue d'appeler state.discard_pending_draft(&session_id) PUIS
// state.store().delete_draft(session_id).await — mais discard plante desormais le
// tombstone, donc l'ordre des deux commandes store n'a plus d'importance pour la
// correction: aucun upsert observe par le delete n'est jamais emis.
```

**Notes.** Tests in-crate (`take_dirty_drafts` privé) : `delete_during_in_flight_flush_never_resurrects_the_draft` (reproduit l'interleaving F10, assert `store().draft()==None`), `retyping_after_delete_clears_the_tombstone_and_persists`, `flush_persists_a_live_draft`. `deleted_at.clear()` au drain est sain (un tombstone antérieur a déjà retiré le draft de `dirty`). Génération u64 ne déborde jamais (≤2 writes/s). Mutex jamais tenu à travers un `.await`. Perf : 1 lock + 1 lookup HashMap par draft, pas de fsync supplémentaire.

#### F20 — flush en une seule transaction (1 fsync pour le lot)
**Approche.** Au lieu de N upserts séquentiels (N fsync FULL), persister tout le tick en UNE transaction SQLite via une nouvelle commande store `UpsertDrafts`. Côté hub, `flush_drafts` agrège les drafts drainés en un `Vec<DraftWrite>` trié « plus frais d'abord », en préservant le verrou tombstone (F10) vérifié en bloc sous un seul lock. Coût = 1 fsync + N inserts bon marché.

```rust
// ============================================================================
// crates/fluence-store/src/types.rs  — new batch-write shape (P0 behind SecretString)
// ============================================================================

/// One draft write in a batch flush. `text` is **P0** (behind
/// `SecretString`, so `Debug` never shows it).
#[derive(Debug)]
pub struct DraftWrite {
    /// Session the draft belongs to.
    pub session_id: String,
    /// Draft text (P0).
    pub text: SecretString,
    /// Caret position (Unicode scalar values).
    pub caret: u32,
    /// Client-side timestamp of the last keystroke, microseconds.
    pub updated_at_micros: u64,
}

// ============================================================================
// crates/fluence-store/src/actor.rs
// ============================================================================

// import:
use crate::types::{AccessEntry, DeviceRecord, DraftRecord, DraftWrite, NewAccessEntry, NewDevice};

// new Command variant (in `enum Command`):
    /// Insert/replace many drafts in a single transaction (one fsync for
    /// the whole batch — the autosave flush path, D-2.6).
    UpsertDrafts {
        /// Drafts to persist, in the order they should be written.
        drafts: Vec<DraftWrite>,
        /// Result channel.
        reply: Reply<()>,
    },

// new dispatch arm (in `fn dispatch`):
        Command::UpsertDrafts { drafts, reply } => {
            let _ = reply.send(upsert_drafts(conn, &drafts));
        }

/// Persists a whole batch of drafts inside a single transaction, so the
/// `synchronous=FULL` fsync cost is paid **once** for the batch instead of
/// once per draft. This bounds the autosave flush duration regardless of
/// how many sessions are dirty (D-2.6): a flusher tick no longer stretches
/// linearly with the session count, which kept the «&nbsp;≤ 1 s lost&nbsp;»
/// window from blowing up under a burst of distinct sessions. An empty
/// batch is a no-op (no transaction, no fsync). The whole batch commits or
/// rolls back atomically — a partial flush never leaves the store in a
/// state the loss-bound reasoning does not cover.
fn upsert_drafts(conn: &mut Connection, drafts: &[DraftWrite]) -> Result<(), StoreError> {
    if drafts.is_empty() {
        return Ok(());
    }
    let tx = conn.transaction()?;
    {
        let mut statement = tx.prepare_cached(
            "INSERT INTO drafts (session_id, text, caret, updated_at_micros)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(session_id) DO UPDATE
             SET text = excluded.text, caret = excluded.caret,
                 updated_at_micros = excluded.updated_at_micros",
        )?;
        for draft in drafts {
            statement.execute(params![
                draft.session_id,
                draft.text.expose_secret(),
                draft.caret,
                draft.updated_at_micros,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

// ============================================================================
// crates/fluence-store/src/lib.rs
// ============================================================================

// re-export:
pub use types::{AccessEntry, DeviceRecord, DraftRecord, DraftWrite, NewAccessEntry, NewDevice};

// new async handle method (in `impl Store`):
    /// Inserts or replaces many drafts atomically in a single transaction.
    /// **P0 content.** One fsync covers the whole batch, which is what keeps
    /// the autosave flush bounded under many dirty sessions (D-2.6); an
    /// empty batch is a cheap no-op.
    ///
    /// # Errors
    ///
    /// [`StoreError`] on database failure or closed store.
    pub async fn upsert_drafts(&self, drafts: Vec<DraftWrite>) -> Result<(), StoreError> {
        self.call(|reply| Command::UpsertDrafts { drafts, reply })
            .await
    }

// ============================================================================
// crates/fluence-hub/src/state.rs  — flush_drafts now batches the whole tick
// (integre avec le modele tombstone/generation F10 deja present dans le fichier)
// ============================================================================

    /// Flushes all dirty drafts to the store. Called by the periodic
    /// flusher and by graceful shutdown.
    ///
    /// The whole tick goes to the store as **one** transaction — one fsync
    /// for every draft instead of one per draft. This bounds the flush
    /// duration regardless of how many sessions are dirty: a burst of
    /// distinct sessions (a buggy or adversarial local client) can no longer
    /// stretch a tick past the «&nbsp;≤ 1 s lost&nbsp;» window and starve a
    /// legitimately-typed session of its autosave (D-2.6). Cost is now
    /// `1 fsync + N cheap row writes`, not `N fsyncs`.
    ///
    /// Upserts are suppressed when the session was closed since it was
    /// buffered (`tombstone >= generation`): the `DELETE` wins, so a freshly
    /// closed conversation is never written back into the encrypted store
    /// (F10 / SPEC §9.A). The check runs once for the batch, under the
    /// buffer lock and before the asynchronous store call — the lock only
    /// spans `HashMap` lookups, never an `.await`.
    pub async fn flush_drafts(&self) {
        let drained = self.take_dirty_drafts();
        if drained.is_empty() {
            return;
        }
        let mut writes: Vec<fluence_store::DraftWrite> = {
            let drafts = lock(&self.0.drafts);
            drained
                .into_iter()
                .filter(|d| {
                    // Keep the write unless a delete observed this draft (or
                    // a newer one) since it was buffered (F10 / SPEC §9.A).
                    drafts
                        .deleted_at
                        .get(&d.session_id)
                        .is_none_or(|&tombstone| tombstone < d.generation)
                })
                .map(|d| fluence_store::DraftWrite {
                    session_id: d.session_id,
                    text: d.draft.text,
                    caret: d.draft.caret,
                    updated_at_micros: d.draft.updated_at_micros,
                })
                .collect()
        };
        if writes.is_empty() {
            return;
        }
        // Freshest keystroke first: under a stale-session flood the actively
        // typed session is committed (and fsynced, atomically with the rest)
        // at the head of the batch rather than behind thousands of others.
        writes.sort_unstable_by(|a, b| b.updated_at_micros.cmp(&a.updated_at_micros));
        if let Err(error) = self.store().upsert_drafts(writes).await {
            // The batch rolled back atomically; `StoreError` carries no P0.
            tracing::error!(%error, "draft flush failed");
        }
    }
```

**Notes.** Ce patch reprend le modèle tombstone/génération de F10 ; intégrer F10 d'abord, puis F20. Atomicité : `prepare_cached` + un seul `tx.commit()` = 1 fsync FULL pour tout le lot ; rollback complet en cas d'erreur. P0 : `DraftWrite.text` reste `SecretString`, `expose_secret()` uniquement vers SQLCipher, `StoreError` sans contenu. Tests store : `batch_upsert_persists_every_draft_in_one_transaction` (500 drafts), `empty_batch_upsert_is_a_noop` ; kill-test `flush_stays_bounded_under_a_flood_of_dirty_sessions` (3000 sessions + frappe légitime + kill -9 mid-flush → perte ≤1 s). En régime normal (≤2 writes/s) coût identique à avant ; sous flood, durée passe de O(N) à O(1) fsync. *Attention : intégrer F01 ET F10/F20 sur le même `flush_drafts` exige un merge manuel — F01 utilise le modèle HashMap simple, F10/F20 le modèle DraftBuffer/tombstone. Le modèle tombstone (F10/F20) est le plus complet ; porter la logique « clear conditionnel sur upsert confirmé » de F01 dans le chemin batch reste à faire si l'on veut aussi la non-perte sur erreur IO partielle.*

### Cluster B — Bornage des ressources (F09, F26, F15, G7)

Cause racine commune : aucune politique de plafond/rétention/quota sur des collections pilotées par l'input d'un device local.

#### F09 — triple bornage RAM + disque des drafts
**Approche.** (1) cap dur de taille du texte validé AVANT toute allocation P0 ; (2) cap de cardinalité sur `dirty_drafts` — une session déjà bufferisée est toujours mise à jour (clavier jamais refusé), seule la création d'une N+1ᵉ session déclenche un flush synchrone immédiat ; (3) bornage disque via purge TTL des drafts orphelins côté store.

```rust
// ============================================================================
// BLOC 1 — crates/fluence-hub/src/state.rs
// Cardinality cap on the in-RAM dirty-draft buffer + flush-on-overflow.
// ============================================================================

/// Hard cap on the maximum draft text the hub will buffer, in bytes.
/// Axum already limits a request body to 2 MiB by default, but the draft
/// text is P0 held in clear in RAM: we keep it small on purpose. A real
/// composed message is a few hundred bytes; 64 KiB is orders of magnitude
/// above any legitimate draft while making a flood of giant `SecretString`s
/// impossible. Enforced before any P0 allocation (see `put_draft`).
pub const MAX_DRAFT_TEXT_BYTES: usize = 64 * 1024;

/// Hard cap on the number of distinct sessions whose unflushed draft the
/// hub keeps in RAM at once. A household has a handful of concurrent
/// composing sessions; this ceiling is far above any real usage yet bounds
/// a Control device that loops `PUT` under fresh session ids (F09). Hitting
/// it never drops or blocks a write — it forces an immediate flush so the
/// P0 leaves RAM for the encrypted store (loss bound stays ≤ 1 s, D-2.6).
pub const MAX_DIRTY_DRAFTS: usize = 256;

// (unchanged) `Inner.dirty_drafts: Mutex<HashMap<String, PendingDraft>>`

/// Buffers a draft write (overwrites a previous unflushed one — only the
/// latest state matters). Returns `true` when the buffer overflowed its
/// cardinality cap and the caller must flush *now* to bring it back down
/// (F09): updating an already-buffered session never overflows, so the
/// keyboard path of an active session is never throttled — only the
/// appearance of a brand-new, never-flushed session beyond the cap does.
#[must_use]
pub fn buffer_draft(&self, session_id: String, draft: PendingDraft) -> bool {
    let mut dirty = lock(&self.0.dirty_drafts);
    let is_new_session = !dirty.contains_key(&session_id);
    dirty.insert(session_id, draft);
    is_new_session && dirty.len() > MAX_DIRTY_DRAFTS
}

// (`take_dirty_drafts`, `pending_draft`, `discard_pending_draft`,
//  `flush_drafts`, `spawn_draft_flusher` stay exactly as they are: the
//  drain-everything-then-upsert flusher already empties the map every
//  500 ms, so the overflow path just brings that flush forward.)

// ============================================================================
// BLOC 2 — crates/fluence-hub/src/api/sessions.rs
// Validate text length BEFORE building the P0 SecretString; flush on
// cardinality overflow. No P0 ever appears in the error body or logs.
// ============================================================================

/// `PUT /api/v1/sessions/{id}/draft`: buffers the keystroke state; the
/// periodic flusher persists it (≤ 1 s loss bound, D-2.6). The text becomes
/// a `SecretString` at the boundary — P0 never travels bare through the hub.
///
/// Two guards keep an authenticated Control device from exhausting hub
/// resources (F09): the draft text is rejected past `MAX_DRAFT_TEXT_BYTES`
/// (a 422 carrying *no* P0), and an overflow of the per-session buffer cap
/// forces an immediate flush so RAM use stays bounded.
pub async fn put_draft(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(draft): Json<Draft>,
) -> Response {
    // Reject oversized text before it is ever wrapped as P0. `len()` is the
    // byte length; the error detail mentions only the limit, never the text.
    if draft.text.len() > crate::state::MAX_DRAFT_TEXT_BYTES {
        return problem_response(
            fluence_protocol::error::ErrorCode::ValidationFailed,
            Some(format!(
                "draft text exceeds the {}-byte limit",
                crate::state::MAX_DRAFT_TEXT_BYTES
            )),
        );
    }

    let updated_at_micros = u64::try_from(chrono::Utc::now().timestamp_micros()).unwrap_or(0);
    let overflow = state.buffer_draft(
        session_id,
        PendingDraft {
            text: SecretString::from(draft.text),
            caret: draft.caret,
            updated_at_micros,
        },
    );
    // Too many distinct unflushed sessions: drain to disk right away. This
    // is the same drain the periodic flusher does, only brought forward —
    // it never blocks the keystroke (the write is already buffered) and the
    // P0 lands in the encrypted store within this request, not in RAM.
    if overflow {
        state.flush_drafts().await;
    }
    StatusCode::NO_CONTENT.into_response()
}

// ============================================================================
// BLOC 3 — crates/fluence-store/src/schema.rs
// Append-only migration v1 → v2: index drafts by recency so the TTL purge
// is a cheap range scan, not a full-table walk.
// ============================================================================

const MIGRATIONS: &[&str] = &[
    // v0 → v1: initial schema. (unchanged — never edit a shipped migration)
    "
    CREATE TABLE devices ( /* … unchanged … */ );
    CREATE TABLE drafts (
        session_id         TEXT PRIMARY KEY,
        text               TEXT NOT NULL,
        caret              INTEGER NOT NULL,
        updated_at_micros  INTEGER NOT NULL
    );
    CREATE TABLE access_journal ( /* … unchanged … */ );
    CREATE TABLE profiles ( /* … unchanged … */ );
    ",
    // v1 → v2: bound the on-disk drafts table. Drafts have no natural
    // expiry today (no FK to a sessions table, no DELETE except on explicit
    // session close), so a Control device looping PUTs under fresh ids would
    // grow this table without limit (F09). The index makes the periodic TTL
    // purge (see actor::purge_stale_drafts) an indexed range delete.
    "
    CREATE INDEX idx_drafts_updated_at ON drafts (updated_at_micros);
    ",
];

// ============================================================================
// BLOC 4 — crates/fluence-store/src/actor.rs
// New `PurgeStaleDrafts` command + handler. Bounds disk growth by deleting
// drafts no client has touched within the TTL. Metadata only — no P0.
// ============================================================================

// in `enum Command`:
    /// Delete drafts untouched since `older_than_micros` (TTL purge, F09).
    PurgeStaleDrafts {
        /// Cutoff: drafts with `updated_at_micros < older_than_micros` die.
        older_than_micros: u64,
        /// Result channel: number of drafts purged (metadata, never P0).
        reply: Reply<u64>,
    },

// in `dispatch`:
        Command::PurgeStaleDrafts {
            older_than_micros,
            reply,
        } => {
            let _ = reply.send(purge_stale_drafts(conn, older_than_micros));
        }

/// Deletes every draft whose last keystroke is older than the cutoff.
/// Uses the `idx_drafts_updated_at` index. Returns the count purged — a
/// plain number, safe to log (no P0 ever leaves this function).
fn purge_stale_drafts(conn: &Connection, older_than_micros: u64) -> Result<u64, StoreError> {
    let purged = conn.execute(
        "DELETE FROM drafts WHERE updated_at_micros < ?1",
        params![older_than_micros],
    )?;
    Ok(purged as u64)
}

// ============================================================================
// BLOC 5 — crates/fluence-store/src/lib.rs
// Public async handle for the purge.
// ============================================================================

/// Purges drafts no client has touched within the TTL window (F09 disk
/// bound). Returns the number of drafts removed — metadata only, never P0.
///
/// # Errors
///
/// [`StoreError`] on database failure or closed store.
pub async fn purge_stale_drafts(&self, older_than_micros: u64) -> Result<u64, StoreError> {
    self.call(|reply| Command::PurgeStaleDrafts {
        older_than_micros,
        reply,
    })
    .await
}

// ============================================================================
// BLOC 6 — crates/fluence-hub/src/state.rs (purger task) + lib.rs (spawn)
// Periodic on-disk purge. Independent of any AI/worker health — the
// keyboard guarantee is untouched (SPEC §2.C).
// ============================================================================

// in state.rs:

/// Drafts untouched for this long are purged from disk. Generous: it only
/// reclaims abandoned/orphaned drafts (e.g. fabricated session ids from a
/// flood) — a live conversation re-touches its draft far more often. Tune
/// freely; it never affects an active session.
pub const DRAFT_DISK_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60); // 7 days
/// How often the disk purge runs (cheap indexed delete, off the hot path).
pub const DRAFT_PURGE_PERIOD: Duration = Duration::from_secs(60 * 60); // hourly

/// Spawns the periodic on-disk stale-draft purger (runs until the hub
/// stops). Bounds the `drafts` table independently of session lifecycle
/// (F09); a store error is logged and retried next tick — never fatal.
pub fn spawn_draft_purger(&self) {
    let state = self.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(DRAFT_PURGE_PERIOD);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let ttl_micros = u64::try_from(DRAFT_DISK_TTL.as_micros()).unwrap_or(u64::MAX);
            let now_micros = u64::try_from(Utc::now().timestamp_micros()).unwrap_or(0);
            let cutoff = now_micros.saturating_sub(ttl_micros);
            match state.store().purge_stale_drafts(cutoff).await {
                Ok(0) => {}
                Ok(purged) => tracing::debug!(purged, "stale drafts purged"),
                Err(error) => tracing::warn!(%error, "stale-draft purge failed"),
            }
        }
    });
}

// in lib.rs `start`, right after `state.spawn_draft_flusher();`:
    state.spawn_draft_purger();
```

**Notes.** **Conflit d'intégration majeur :** ce patch suppose le modèle `dirty_drafts: HashMap` simple, mais F10/F20 remplacent ce champ par `DraftBuffer`. À l'intégration, porter `buffer_draft -> bool` (is_new + len>cap) sur le modèle retenu (tombstone). Le `buffer_draft` retourne désormais `bool` (`#[must_use]`) — seul appelant `sessions.rs:51`. Invariant clavier : aucune branche ne bloque l'écriture d'une session active ; le 422 ne concerne que du texte aberrant (>64 KiB). `'as u64'` → préférer `u64::try_from(purged).unwrap_or(u64::MAX)` pour clippy-pedantic. Migration v1→v2 append-only, idempotente via `user_version`. Tests : `buffer_draft` renvoie false pour session répétée ; PUT >64 KiB → 422 sans le texte ; boucle N>MAX_DIRTY_DRAFTS UUID → map bornée + hub toujours 204 ; `purge_stale_drafts` ne supprime que l'ancien.

#### F26 — journal borné par construction (trim dans la même transaction)
**Approche.** Appliquer la rétention DANS `journal_append` : co-localiser l'INSERT et un TRIM (`DELETE id <= max(id) - budget`) dans une seule transaction (donc un seul fsync). La table ne peut plus dépasser `JOURNAL_MAX_ROWS=5000`, ce qui neutralise saturation disque ET amplification fsync. Aucune commande ni sweeper supplémentaire.

```rust
// === crates/fluence-store/src/actor.rs ===

// (1) New constant, placed right after `type Reply<R> = ...;`

/// Hard cap on retained access-journal rows (ADR-0005 §5; F26).
///
/// The journal is *append-only metadata* written on uncontrolled paths —
/// notably `auth.rejected`, which an unauthenticated loopback client can
/// hammer (`auth.rs`, `ws.rs`). Without a bound the table grows forever:
/// it fills the home disk *and* — worse, because it bites first — floods
/// the single store connection's fsync queue, starving the draft flusher
/// and threatening the «&nbsp;≤ 1 s lost&nbsp;» guarantee (D-2.6). Trimming
/// on every append makes growth structurally impossible and keeps the WAL
/// and its checkpoints small, so the keyboard path stays fast. 5 000 rows
/// is far more history than the caregiver UI ever shows yet trivial on
/// disk (< 1 MB encrypted).
const JOURNAL_MAX_ROWS: i64 = 5_000;

// (2) Replace the body of `journal_append`. Note the signature changes to
// `&mut Connection` (needed for `conn.transaction()`); the dispatch call
// site `journal_append(conn, &entry)` already passes a `&mut Connection`,
// so it compiles unchanged.

/// Appends a journal entry and trims the table back under
/// [`JOURNAL_MAX_ROWS`] in the *same* transaction, so the access journal
/// is bounded by construction (F26).
///
/// Insert and trim share one commit — hence one fsync — so a flood of
/// `auth.rejected` writes cannot multiply IO on the connection the draft
/// flusher depends on. The trim deletes by `id` (the rowid: an integer
/// primary key, already indexed), and `id` is `AUTOINCREMENT`, so the
/// high-water mark keeps rising across deletes and reopens. In steady
/// state at most one row is evicted per append, so this is amortized
/// O(1); the first append after a migration from an over-budget table
/// pays a one-off bulk delete.
fn journal_append(conn: &mut Connection, entry: &NewAccessEntry) -> Result<(), StoreError> {
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO access_journal (at, device_id, action, detail) VALUES (?1, ?2, ?3, ?4)",
        params![
            Utc::now().to_rfc3339(),
            entry.device_id,
            entry.action,
            entry.detail
        ],
    )?;
    // Keep only the newest JOURNAL_MAX_ROWS rows. `max(id)` is the
    // monotonic high-water mark; anything more than the budget below it is
    // stale. NULL-safe: on an (impossible here, post-insert) empty table
    // `max(id)` is NULL and the predicate matches nothing.
    tx.execute(
        "DELETE FROM access_journal
         WHERE id <= (SELECT max(id) FROM access_journal) - ?1",
        params![JOURNAL_MAX_ROWS],
    )?;
    tx.commit()?;
    Ok(())
}

// === crates/fluence-store/tests/store_roundtrips.rs (regression test) ===

#[tokio::test]
async fn journal_is_bounded_under_a_flood() {
    // F26: an unauthenticated loopback client can hammer `auth.rejected`.
    // The journal must stay bounded so it neither fills the home disk nor
    // floods the store connection the draft flusher shares (D-2.6). We
    // assert the row cap holds and the *newest* entries survive eviction.
    let dir = tempfile::tempdir().expect("tempdir");
    let config = config_in(&dir);
    let store = Store::open(config.clone()).await.expect("open");

    let total = 5_000 + 250;
    for i in 0..total {
        store
            .journal_append(NewAccessEntry {
                device_id: None,
                action: "auth.rejected".into(),
                detail: Some(format!("seq={i}")),
            })
            .await
            .expect("append");
    }

    let recent = store.journal_recent(10_000).await.expect("recent");
    assert!(recent.len() <= 5_000, "journal must stay bounded, got {}", recent.len());
    assert_eq!(
        recent[0].detail.as_deref(),
        Some(format!("seq={}", total - 1).as_str()),
        "the newest entry must survive eviction"
    );

    store.close().await.expect("close");
    let reopened = Store::open(config).await.expect("reopen");
    let after = reopened.journal_recent(10_000).await.expect("recent");
    assert!(after.len() <= 5_000, "bound must hold after reopen");
}
```

**Notes.** `id` doit être `INTEGER PRIMARY KEY AUTOINCREMENT` (high-water mark monotone même après DELETE/reopen). Signature `&mut Connection` — le call site `dispatch` passe déjà `&mut`. Atomicité : insert+trim dans une transaction = un seul fsync (on borne la charge IO, on ne l'augmente pas). P0 : aucun contenu journalisé. Dette à tracer : rate-limit/connection-limit global sur le routeur loopback en amont (le fix borne la conséquence, pas la cause réseau) ; éventuellement connexion store dédiée au journal en `synchronous=NORMAL`.

#### F15 — quota de connexions WS par device + global (garde RAII)
**Approche.** Comptabiliser les connexions WS ouvertes dans `AppState` avec un plafond par device ET global, vérifiés atomiquement sous un seul Mutex avant `on_upgrade`. La place est réservée par un garde RAII (`WsConnectionGuard`) dont le `Drop` décrémente le compteur sur TOUTES les sorties de `serve()`. Au-delà du quota : 429 RateLimited + entrée de journal, sans jamais ouvrir la tâche ni le `broadcast::Receiver`.

```rust
// ============================================================================
// crates/fluence-hub/src/state.rs
// ============================================================================
//
// 1) New imports at the top of the file (next to the existing ones):

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
// ... existing imports unchanged ...

// 2) New tunables, placed beside the other SPEC constants (after
//    DRAFT_FLUSH_PERIOD):

/// Concurrent `/ws` connections a single device may hold open. A real
/// client opens one channel; a handful covers honest reconnect races and
/// multiple windows. Past this, a paired-but-misbehaving device is curbed
/// before it can starve file descriptors / RAM and take the keyboard path
/// down (SPEC §2.C — "the keyboard always speaks").
pub const WS_MAX_PER_DEVICE: u32 = 8;

/// Hub-wide ceiling on concurrent `/ws` connections, independent of how
/// many devices contribute. A backstop for the household so no single
/// device — even within its own quota — can exhaust the process. Sits well
/// under a default 1024-FD soft limit, leaving headroom for the store,
/// worker pipes and the listener.
pub const WS_MAX_TOTAL: u32 = 128;

// 3) The bookkeeping holder (private to the module), kept with `Inner`.
//    `total` is tracked alongside the per-device map so both ceilings are
//    decided under one lock, never recomputed from the map.

/// Open-`/ws` accounting: per-device counts and their running total, both
/// guarded by the same mutex so an admission decision is atomic.
#[derive(Default)]
struct WsCounters {
    per_device: HashMap<String, u32>,
    total: u32,
}

// 4) Add the field to `Inner`:

struct Inner {
    config: HubConfig,
    store: Store,
    bus: EventBus,
    started_at: DateTime<Utc>,
    pairing: Mutex<Option<PairingWindow>>,
    dirty_drafts: Mutex<HashMap<String, PendingDraft>>,
    workers: Mutex<Vec<Arc<WorkerHandle>>>,
    // Concurrent `/ws` connections, per device and in total (DoS guard).
    ws_counters: Mutex<WsCounters>,
}

// 5) Initialize it in `AppState::new` (add the field to the struct literal):

        Self(Arc::new(Inner {
            config,
            store,
            bus,
            started_at: Utc::now(),
            pairing: Mutex::new(None),
            dirty_drafts: Mutex::new(HashMap::new()),
            workers: Mutex::new(Vec::new()),
            ws_counters: Mutex::new(WsCounters::default()),
        }))

// 6) The RAII guard + admission method, added in `impl AppState`:

    /// Reserves a `/ws` slot for `device_id` if both the per-device and
    /// the hub-wide ceilings allow it, returning a guard that releases the
    /// slot on drop. `None` means the device is at quota (or the hub is
    /// saturated) and the connection must be refused *before* upgrade — no
    /// task, no `broadcast::Receiver`, no file descriptor is committed.
    ///
    /// Admission is decided under a single lock so two simultaneous
    /// upgrades cannot both pass the last slot.
    #[must_use]
    pub fn try_acquire_ws(&self, device_id: &str) -> Option<WsConnectionGuard> {
        let mut counters = lock(&self.0.ws_counters);
        if counters.total >= WS_MAX_TOTAL {
            return None;
        }
        let device_count = counters.per_device.get(device_id).copied().unwrap_or(0);
        if device_count >= WS_MAX_PER_DEVICE {
            return None;
        }
        counters.total += 1;
        counters.per_device.insert(device_id.to_owned(), device_count + 1);
        Some(WsConnectionGuard {
            state: self.clone(),
            device_id: device_id.to_owned(),
        })
    }

    /// Releases one `/ws` slot held by `device_id` (called only by
    /// [`WsConnectionGuard`] on drop). Saturating arithmetic and entry
    /// removal keep the map bounded and underflow-free even if a count is
    /// somehow off.
    fn release_ws(&self, device_id: &str) {
        let mut counters = lock(&self.0.ws_counters);
        counters.total = counters.total.saturating_sub(1);
        if let std::collections::hash_map::Entry::Occupied(mut entry) =
            counters.per_device.entry(device_id.to_owned())
        {
            let remaining = entry.get().saturating_sub(1);
            if remaining == 0 {
                entry.remove();
            } else {
                *entry.get_mut() = remaining;
            }
        }
    }

// 7) The guard type itself, placed near `PendingDraft` (module level):

/// Releases its reserved `/ws` slot on drop. Tying the slot's lifetime to
/// a stack value guarantees the count is decremented on *every* exit of the
/// connection task — clean close, network error, or panic — so a flood of
/// dropped connections can never leak the ceiling.
pub struct WsConnectionGuard {
    state: AppState,
    device_id: String,
}

impl Drop for WsConnectionGuard {
    fn drop(&mut self) {
        self.state.release_ws(&self.device_id);
    }
}


// ============================================================================
// crates/fluence-hub/src/api/ws.rs
// ============================================================================
//
// 8) In `upgrade`, after the device is resolved and before `on_upgrade`,
//    reserve a slot. On refusal, emit a journal entry (device_id is not P0)
//    and return a 429 — never open the task or subscribe. The keyboard path
//    is untouched: this handler only governs the event channel.

    let allowed = allowed_topics(device.scope);
    let granted: Vec<Topic> = parse_topics(&query.topics)
        .into_iter()
        .filter(|topic| allowed.contains(topic))
        .collect();

    // Cap concurrent `/ws` per device and hub-wide so a paired-but-rogue
    // device cannot exhaust FDs/RAM and starve the keyboard (SPEC §2.C).
    // Reserved before upgrade: a refusal commits no task and no bus
    // subscription. The guard moves into `serve` and releases on drop.
    let Some(ws_guard) = state.try_acquire_ws(&device.device_id) else {
        state
            .journal(
                "ws.rejected",
                Some(device.device_id.clone()),
                Some("ws connection quota reached"),
            )
            .await;
        return problem_response(fluence_protocol::error::ErrorCode::RateLimited, None);
    };

    upgrade.on_upgrade(move |socket| serve(socket, state, granted, ws_guard))

// 9) Thread the guard through `serve` so it lives exactly as long as the
//    connection loop. `_guard` is held for its drop side effect; binding it
//    (not `_`) is essential — `let _ = ...` would drop it immediately.

/// Connection loop: hello, then fan out bus frames filtered by granted
/// topics, with heartbeat pings. `_guard` releases the connection's `/ws`
/// slot when this task ends, by any path.
async fn serve(
    mut socket: WebSocket,
    state: AppState,
    granted: Vec<Topic>,
    _guard: WsConnectionGuard,
) {
    // ... body unchanged ...
}

// 10) Update the import block of ws.rs to pull the guard in:

use crate::state::{AppState, WsConnectionGuard};
```

**Notes.** **Conflit d'intégration :** ce patch montre `Inner` avec `dirty_drafts: HashMap` ; à fusionner avec F10/F20 (DraftBuffer) — n'ajouter que le champ `ws_counters` au modèle retenu. Liaison `_guard` (pas `_`) impérative sinon Drop immédiat. Admission sous un seul lock (pas de race sur le dernier slot). Le `Receiver` n'est jamais créé en cas de refus (le bug original). Invariant clavier : le quota ne touche que /ws ; le refus est explicite (429). Tests : `ws_per_device_quota_is_enforced`, `ws_flood_does_not_kill_the_keyboard` (PUT draft toujours 2xx sous flood), `ws_global_quota_caps_the_hub`. Dette : rendre les seuils configurables via `HubConfig` en Phase 5.

#### G7 — limite de corps explicite
*(Pas de patch fourni dans REMEDIATIONS — recoupe F09/F15 ; durcissement = ajouter `.layer(DefaultBodyLimit::max(MAX_DRAFT_TEXT_BYTES))` explicite sur le routeur plutôt que dépendre du défaut implicite d'axum. À implémenter avec F09.)*

### Cluster C — At-rest & exposition réseau (F06, F08)

#### F06 — garde-fou runtime sur la clé en clair co-localisée
**Approche.** Ajouter un avertissement explicite au démarrage dès que la clé vit dans un fichier en clair, avec un ton ESCALADE quand ce fichier est co-localisé avec `store.db`. Le log ne contient que des chemins/booléens (jamais le contenu de la clé ni de P0).

```rust
// ===========================================================================
// crates/fluence-hub/src/lib.rs
// ===========================================================================
//
// In `start(...)`, build the key source and emit the at-rest warning BEFORE
// opening the store, so an operator running a headless / file-key install is
// told — every boot — that disk-theft protection is degraded. The store path
// is computed once and shared, so the warning can detect co-location exactly.

pub async fn start(config: HubConfig) -> Result<RunningHub, HubError> {
    let store_path = config.data_dir.join("store.db");
    let key = store_key_source(&config);
    warn_if_at_rest_degraded(&key, &store_path);

    let store = Store::open(StoreConfig {
        path: store_path,
        key,
    })
    .await?;

    let bus = EventBus::new();
    let state = AppState::new(config, store, bus);
    state.spawn_draft_flusher();
    // ... unchanged below ...
    // (rest of `start` is untouched)
    # unreachable!()
}

/// Chooses where the store master key lives. An explicit `store_key_file`
/// always wins; otherwise Windows uses the OS keystore (DPAPI) and other
/// platforms use a 0600 file in the data dir (ADR-0005; the headless Linux
/// hub has no desktop keystore).
///
/// Note: [`KeySource::File`] stores the SQLCipher key as plaintext hex with no
/// passphrase/KDF (D-9.1: max entropy, fast start). It is therefore only as
/// strong as the filesystem ACL: if the key file is copied alongside
/// `store.db`, AES-256 at rest no longer protects against disk theft. The OS
/// keystore (Windows DPAPI) is the production default and ties the key to the
/// user login. See [`warn_if_at_rest_degraded`].
fn store_key_source(config: &HubConfig) -> KeySource {
    if let Some(path) = &config.store_key_file {
        return KeySource::File(path.clone());
    }
    if cfg!(windows) {
        KeySource::Keyring {
            service: "fluence".to_owned(),
            entry: "store-key".to_owned(),
        }
    } else {
        KeySource::File(config.data_dir.join("store.key"))
    }
}

/// Emits an explicit, recurring warning when the master key lives in a
/// plaintext file rather than the OS keystore — disk-theft coverage
/// (SPEC §9.A « voleur du PC (chiffrement) ») is degraded, never silently.
///
/// The warning escalates when the key file sits in the SAME directory as
/// `store.db`: copying that directory then hands an attacker both the
/// ciphertext and its key, neutralising at-rest encryption. The documented
/// cross-machine recovery path is the printable rescue kit (QR + phrase,
/// SPEC §9.A), never a co-located key file.
///
/// Logs only paths and booleans — never the key material (P1 secret) nor any
/// draft content (P0). Paths are operator-facing config, not P0 (SPEC §9.A).
fn warn_if_at_rest_degraded(key: &KeySource, store_path: &std::path::Path) {
    let KeySource::File(key_path) = key else {
        // OS keystore: key is login-bound, disk theft alone cannot decrypt.
        return;
    };
    if key_file_is_colocated(key_path, store_path) {
        tracing::warn!(
            key_path = %key_path.display(),
            store_path = %store_path.display(),
            "master key is a PLAINTEXT FILE co-located with the store: \
             copying the data directory exposes both database and key \
             (at-rest encryption no longer protects against disk theft). \
             Use the OS keystore, relocate the key off this volume, or rely \
             on the printable rescue kit for cross-machine recovery (SPEC §9.A)"
        );
    } else {
        tracing::warn!(
            key_path = %key_path.display(),
            "master key is a plaintext file (no OS keystore on this install): \
             at-rest disk-theft protection depends on filesystem permissions \
             only — keep this file off shared/backup/sync volumes (SPEC §9.A)"
        );
    }
}

/// True when the key file and the store would land in the same directory, so
/// that a single folder copy leaks both. Canonicalises each parent when it
/// already exists (resolves symlinks/`.`/`..`/case on the real FS); falls back
/// to a lexical parent comparison when a path does not yet exist (first boot,
/// before the data dir is created). Conservative: any uncertainty that the
/// directories *might* match resolves to `true` so the louder warning wins.
fn key_file_is_colocated(key_path: &std::path::Path, store_path: &std::path::Path) -> bool {
    let key_dir = key_path.parent();
    let store_dir = store_path.parent();
    match (key_dir, store_dir) {
        (Some(k), Some(s)) => {
            let kc = std::fs::canonicalize(k);
            let sc = std::fs::canonicalize(s);
            match (kc, sc) {
                (Ok(k), Ok(s)) => k == s,
                // One side not yet on disk: compare lexically and lean loud.
                _ => k == s,
            }
        }
        // A bare filename (no parent) on either side: treat as the same
        // working directory — assume co-located and warn loudly.
        _ => true,
    }
}

// ===========================================================================
// crates/fluence-hub/src/config.rs  — sharpen the field doc (no behaviour change)
// ===========================================================================

    /// Store the master key in a plaintext hex file instead of the OS
    /// keystore — tests and headless installs (SPEC D-9.1 keeps the keystore
    /// as the production default). Disk-theft coverage (SPEC §9.A) then
    /// depends on filesystem permissions alone; keep this file off
    /// shared/backup/sync volumes and prefer the printable rescue kit for
    /// cross-machine recovery. The hub logs a warning at every boot when this
    /// mode is active (louder still when the file sits next to `store.db`).
    pub store_key_file: Option<PathBuf>,

// ===========================================================================
// crates/fluence-hub/tests/  — kill-test-style guard for the new behaviour
// (add to an existing integration test file, e.g. tests/at_rest_warning.rs).
// These tests exercise the pure helpers, so they need no live hub/store.
// `store_key_source`, `warn_if_at_rest_degraded` and `key_file_is_colocated`
// must be reachable from tests: keep them `pub(crate)` and re-exported, or
// move the two assertions below into a `#[cfg(test)] mod tests` inside lib.rs.
// ===========================================================================

#[cfg(test)]
mod at_rest_warning_tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn colocated_key_and_store_in_same_dir_is_detected() {
        let dir = std::path::Path::new("/var/lib/fluence");
        assert!(key_file_is_colocated(
            &dir.join("store.key"),
            &dir.join("store.db"),
        ));
    }

    #[test]
    fn key_in_separate_dir_is_not_flagged_colocated() {
        // Lexical-distinct parents that do not exist on disk: stays quiet-path.
        assert!(!key_file_is_colocated(
            &PathBuf::from("/secrets/fluence/store.key"),
            &PathBuf::from("/var/lib/fluence/store.db"),
        ));
    }

    #[test]
    fn keyring_source_never_triggers_the_warning() {
        // Compile-time: keyring matches the early `return` arm. We assert the
        // helper does not panic and is a no-op for the keystore source.
        let key = KeySource::Keyring {
            service: "fluence".to_owned(),
            entry: "store-key".to_owned(),
        };
        // No log assertion needed: the `let-else` returns immediately.
        warn_if_at_rest_degraded(&key, std::path::Path::new("/var/lib/fluence/store.db"));
    }
}
```

**Notes.** Comportement par défaut inchangé (keystore OS), kill-tests préservés (passent `FLUENCE_STORE_KEY_FILE`). P0-dans-logs respecté : seuls des chemins (P1) et un booléen. `canonicalize` uniquement sur `parent()`, jamais sur le fichier (peut ne pas exister). Test additionnel : capturer la sortie tracing et asserter que le message « PLAINTEXT FILE co-located » apparaît ET qu'aucun hex de 64 chars n'y figure. Dette : wrapping de la clé File par passphrase/KDF ou TPM/keyutils (vrai durcissement at-rest définitif).

#### F08 — refuser un bind non-loopback sans TLS
**Approche.** Garde-fou de transport : tant qu'aucune couche TLS n'est compilée (`TLS_AVAILABLE = false`), refuser tout `listen_addr` non-loopback avec une erreur claire au lieu d'exposer le hub en HTTP clair. Validation dans `HubConfig` (testable), appliquée en fin de `load()` ET au début de `start()` (défense en profondeur). Défaut loopback inchangé.

```rust
// ============================================================================
// crates/fluence-hub/src/config.rs
// ============================================================================

// Add near the top of the file, next to DEFAULT_PORT.

/// Whether this build ships a transport-encryption layer (home mode TLS,
/// SPEC §2.A "HTTPS obligatoire hors loopback"). Wired to `false` until the
/// local-CA TLS stack lands (PLAN task 2.5 / pair.rs "PR C"); flipping it to
/// `true` is the single switch that unlocks non-loopback binds. Keeping the
/// gate behind one constant means the LAN bind and its encryption ship
/// together — never one without the other.
const TLS_AVAILABLE: bool = false;

// Add a new variant to `ConfigError` (alongside InvalidEnv):

    /// A non-loopback listen address was requested while this build has no
    /// transport encryption. Binding it would put pairing codes, device
    /// tokens and P0 draft text in cleartext on the LAN — refused loudly
    /// rather than exposed silently (SPEC §2.A; trust boundary = loopback).
    #[error(
        "refusing to listen on non-loopback address {addr}: this build has \
         no TLS, so home mode (LAN) is unavailable — bind a loopback address \
         (default) or wait for the home-mode TLS release"
    )]
    InsecureBind {
        /// The rejected, non-loopback address. Not P0 (an IP, not user text).
        addr: IpAddr,
    },

// Add this method inside `impl HubConfig` (e.g. right after `apply_env`).
// NB: the error message contains only the IP — never any P0 (draft text,
// tokens, household contents).

    /// Rejects a listen address the current build cannot serve safely.
    ///
    /// The trust boundary is *loopback + household files*. A non-loopback
    /// bind is only sound once transport encryption exists; until then we
    /// fail closed so a stray `FLUENCE_LISTEN_ADDR=0.0.0.0` can never put
    /// tokens or P0 drafts on the wire in cleartext.
    ///
    /// # Errors
    ///
    /// [`ConfigError::InsecureBind`] when `listen_addr` is not a loopback
    /// address and this build has no TLS ([`TLS_AVAILABLE`]).
    pub fn ensure_transport_safe(&self) -> Result<(), ConfigError> {
        if !self.listen_addr.is_loopback() && !TLS_AVAILABLE {
            return Err(ConfigError::InsecureBind {
                addr: self.listen_addr,
            });
        }
        Ok(())
    }

// Apply it at the end of `load`, so the file+env path is guarded too.
// (Replace the tail of `load`.)
//
//        config.apply_env(|name| std::env::var(name).ok())?;
//        config.ensure_transport_safe()?;
//        Ok(config)

// Add these tests to the `tests` module:

    #[test]
    fn non_loopback_bind_is_refused_without_tls() {
        // Trust boundary = loopback. Without TLS, a LAN bind would leak
        // tokens and P0 in cleartext — it must fail closed, loudly.
        let mut config = HubConfig::default();
        config
            .apply_env(|name| {
                (name == "FLUENCE_LISTEN_ADDR").then(|| "0.0.0.0".to_owned())
            })
            .expect("parses");
        let error = config.ensure_transport_safe().expect_err("must refuse");
        assert!(matches!(error, ConfigError::InsecureBind { .. }));
    }

    #[test]
    fn loopback_bind_is_always_allowed() {
        // The default (and only nominal) path must keep working.
        assert!(HubConfig::default().ensure_transport_safe().is_ok());
        let mut config = HubConfig::default();
        config.listen_addr = IpAddr::V6(std::net::Ipv6Addr::LOCALHOST);
        assert!(config.ensure_transport_safe().is_ok());
    }

    #[test]
    fn load_rejects_insecure_listen_addr_from_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "listen_addr = \"0.0.0.0\"\n").expect("write");
        assert!(matches!(
            HubConfig::load(Some(&path)),
            Err(ConfigError::InsecureBind { .. })
        ));
    }


// ============================================================================
// crates/fluence-hub/src/lib.rs
// ============================================================================

// 1) Map the new config error into HubError. Add this variant to `HubError`
//    (alongside Bind / Setup):

    /// The configured listen address cannot be served safely by this build
    /// (non-loopback without TLS). Fail closed — never expose the LAN in
    /// cleartext (SPEC §2.A).
    #[error("insecure listen address: {source}")]
    InsecureListen {
        /// Underlying configuration error.
        #[from]
        source: crate::config::ConfigError,
    },

// 2) Guard `start` before anything binds or even opens the store. This also
//    covers code paths (tests, embeddings) that build `HubConfig` directly
//    and never go through `HubConfig::load`. Insert as the FIRST line of the
//    function body:

pub async fn start(config: HubConfig) -> Result<RunningHub, HubError> {
    // Defense in depth: refuse a non-loopback bind on a build without TLS
    // before opening the store or binding a socket. The keyboard path is
    // unaffected (loopback default); only a deliberate misconfiguration is
    // turned into an explicit, early failure.
    config.ensure_transport_safe()?;

    let store = Store::open(StoreConfig {
        path: config.data_dir.join("store.db"),
        key: store_key_source(&config),
    })
    .await?;
    // ... rest of `start` unchanged ...
}

// Note: `HubError` already derives `thiserror::Error`; the `#[from]` above
// makes `?` on `ensure_transport_safe()` (which yields `ConfigError`) work
// directly. If a `#[from] StoreError` already exists, both `#[from]`s
// coexist fine since their source types differ.
```

**Notes.** `is_loopback()` couvre IPv4 et IPv6 ; `0.0.0.0`/`::` rejetés. Aucune fuite P0 (l'IP n'est pas du texte de draft). Défaut loopback inchangé → kill-tests passent. Bascule future PR C : passer `TLS_AVAILABLE = true` et raffiner pour exiger qu'un cert soit chargé. Perf : comparaison d'enum une fois au boot, impact nul sur < 3 s. **Note d'intégration F06+F08 :** les deux patches modifient `start()` et introduisent chacun `store_key_source` — fusionner en gardant une seule définition de `store_key_source` et en plaçant `ensure_transport_safe()?` en première ligne, puis `warn_if_at_rest_degraded`.

### Cluster D — Bootstrap du token System (G2)

#### G2 — écriture atomique du token avant l'insert + révocation des System stales
**Approche.** Inverser l'ordre pour que l'insert (effet durable) soit l'ultime étape : créer le data_dir, écrire le token ATOMIQUEMENT (temp + rename, perms 0600 avant rename), PUIS insérer le device. Avant tout re-mint, révoquer les devices System encore actifs pour empêcher l'accumulation.

```rust
// SPDX-License-Identifier: Apache-2.0
// crates/fluence-hub/src/lib.rs

/// Creates (first run) or verifies the local system token and writes it
/// to `data_dir/system.token`. The file inherits the data dir's
/// protection, exactly like the store key file.
///
/// Ordering matters for the loss bound and for credential hygiene: the
/// *durable, irreversible* effect (inserting a `System`-scoped device into
/// the encrypted store) is performed **last**, after the fallible
/// filesystem steps. The token plaintext lives only in the file we write,
/// so a store insert that fails after the file is written self-heals on
/// the next boot (file present but unknown to the store -> stale branch
/// re-mints) instead of leaving a live, unusable `System` credential
/// behind. Re-minting first revokes any still-active `System` device, so
/// repeated failed boots never accumulate omnipotent credentials.
async fn ensure_system_token(state: &AppState) -> Result<(), HubError> {
    use fluence_protocol::api::pair::{DeviceKind, Scope};

    let path = state.config().data_dir.join("system.token");
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let known = state
            .store()
            .device_by_token_hash(auth::token_hash(existing.trim()))
            .await?
            .is_some();
        if known {
            return Ok(());
        }
        // Stale file (store reset, or a previous boot that wrote the file
        // but crashed before the insert committed): fall through to mint a
        // fresh one. The replacement below makes the old file's hash
        // unauthenticatable, so no cleanup of the file is needed.
    }

    // Re-minting: any still-active `System` credential from a prior boot is
    // now superseded. Revoke it so a string of failed boots can never grow
    // a set of live, omnipotent System tokens (the lookup path filters on
    // `revoked_at IS NULL`). Best effort — a journal/store hiccup here must
    // not block bringing the keyboard up (SPEC §2.C).
    match state.store().list_devices().await {
        Ok(devices) => {
            for device in devices
                .into_iter()
                .filter(|d| d.scope == Scope::System && d.revoked_at.is_none())
            {
                if let Err(error) = state.store().revoke_device(device.device_id).await {
                    tracing::warn!(%error, "could not revoke a stale system credential");
                }
            }
        }
        Err(error) => tracing::warn!(%error, "could not enumerate devices before re-mint"),
    }

    let token = auth::generate_token();

    // 1) Filesystem first (the fallible step). Write atomically: a torn or
    //    partially written token must never be observable, and a crash
    //    between here and the insert leaves a self-healing "stale file",
    //    never a live orphan credential.
    std::fs::create_dir_all(&state.config().data_dir)
        .map_err(|source| HubError::Setup { context: "create data dir", source })?;
    write_token_atomically(&path, &token)?;

    // 2) Durable store insert last: now the on-disk token has a matching
    //    hash, so the device is immediately presentable by the embedded UI
    //    and the local CLI.
    state
        .store()
        .insert_device(fluence_store::NewDevice {
            device_id: uuid::Uuid::new_v4().to_string(),
            token_hash: auth::token_hash(&token),
            name: "Embedded UI / local CLI".to_owned(),
            kind: DeviceKind::Desktop,
            scope: Scope::System,
        })
        .await?;
    Ok(())
}

/// Writes `token` to `path` atomically: a unique temp sibling, fsync'd and
/// chmod'd 0600 *before* the rename, then `rename` over the target (atomic
/// on the same volume on every supported OS). A reader sees either the old
/// token or the new one — never a half-written file, never default perms.
fn write_token_atomically(path: &std::path::Path, token: &str) -> Result<(), HubError> {
    use std::io::Write as _;

    let setup = |source| HubError::Setup { context: "write system token", source };

    // A pid-tagged sibling avoids clobbering a concurrent writer's temp
    // file; the rename below is what publishes the result.
    let tmp = path.with_extension(format!("token.tmp.{}", std::process::id()));

    // Scoped so the file is closed (flushed) before the rename.
    {
        let mut file = std::fs::File::create(&tmp).map_err(setup)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            // Tighten perms while the bytes are still only in the temp file.
            file.set_permissions(std::fs::Permissions::from_mode(0o600))
                .map_err(setup)?;
        }
        file.write_all(token.as_bytes()).map_err(setup)?;
        // Durability: the token's bytes must hit the platter before the
        // rename names them, so a power loss cannot publish an empty file.
        file.sync_all().map_err(setup)?;
    }

    std::fs::rename(&tmp, path).map_err(|source| {
        // Don't leave the temp file lying around on a rename failure.
        let _ = std::fs::remove_file(&tmp);
        HubError::Setup { context: "write system token", source }
    })?;
    Ok(())
}
```

**Notes.** Aucune donnée P0/token dans les logs (seuls `StoreError` génériques). « Clavier parle toujours » préservé : la révocation est best-effort (warn, pas de `?`) ; les vraies étapes critiques propagent l'erreur (échec dur explicite, conforme). Rename atomique sur le même volume — valide Windows ET Linux. `sync_all()` = un fsync au boot, négligeable. Ce patch introduit `HubError::Setup` (avec champ `context`) — vérifier que le variant existe ou l'ajouter (cohérent avec F30 ci-dessous). Tests : échec d'insert après écriture → un seul device System non révoqué au boot suivant ; kill pendant l'écriture → token absent ou complet (jamais tronqué) ; deux re-mints → exactement un device System `revoked_at IS NULL` ; perms 0600 sur Unix.

### Note transversale sur F30
Le patch G2 introduit le variant `HubError::Setup { context, source }`. C'est aussi la correction directe de **F30** (l'erreur d'écriture de token n'est plus déguisée en `HubError::Bind { addr: port 0 }` mais portée par un variant dédié au setup). Implémenter G2 ferme donc F30.

### Checklist d'actions priorisée

| # | Action | Finding | Sévérité | Effort |
|---|--------|---------|----------|--------|
| 1 | Tombstone + génération pour sérialiser delete-vs-flush | F10 | medium | M |
| 2 | Flush en une transaction (1 fsync/lot) — reprend le modèle F10 | F20 | medium | M |
| 3 | Clear conditionnel sur upsert confirmé (non-perte sur erreur IO) | F01 | medium | M |
| 4 | Cap cardinalité RAM + cap taille texte + purge TTL disque drafts | F09 | medium | L |
| 5 | Trim du journal d'accès dans la transaction d'INSERT | F26 | medium | S |
| 6 | Quota WS par device + global via garde RAII | F15 | medium | M |
| 7 | `DefaultBodyLimit` explicite sur le routeur (avec F09) | G7 | medium | S |
| 8 | Écriture atomique du token + révocation System stales | G2 | medium | M |
| 9 | Variant `HubError::Setup` (clôt aussi le diagnostic trompeur) | F30 | low | S |
| 10 | Garde-fou runtime `warn_if_at_rest_degraded` + doc tranchante | F06 | medium | S |
| 11 | Refuser bind non-loopback sans TLS (`ensure_transport_safe`) | F08 | medium | S |

**Avertissement d'intégration final :** les patches du Cluster A (F01/F10/F20), F09 et F15 modifient TOUS le même `Inner`/`flush_drafts`/`buffer_draft` dans `state.rs`, avec des modèles divergents (HashMap simple vs DraftBuffer/tombstone). Le modèle tombstone (F10/F20) doit être la base ; F01 (clear conditionnel), F09 (cap + bool) et F15 (champ `ws_counters`) sont à reporter manuellement dessus. Ne pas appliquer les blocs « copier mot pour mot » en séquence aveugle — ils se contrediraient sur la définition de `Inner`.