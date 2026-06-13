# ADR-0005 — Architecture d'exécution du hub (Phase 2)

- **Statut** : accepté
- **Date** : 2026-06-13
- **Décisions SPEC liées** : D-2.6 (§2.C), D-2.4 (§2.A), D-9.1 (§9.A) ;
  PLAN Phase 2

## Contexte

La Phase 2 construit le squelette vital : « composer et vocaliser ne
dépendent JAMAIS de la santé des composants IA ». Les choix ci-dessous
fixent comment ce principe devient du code — avec pour priorités la
fiabilité (kill-tests), la lisibilité et le déterminisme des tests.

## Décisions

1. **Crate `fluence-ipc` dédiée** (absente de la liste SPEC §2.B, qui ne se
   prétend pas fermée) : la couche IPC est consommée par le hub ET par tous
   les futurs workers (`fluence-inference`, Phase 4+) — un module privé du
   hub imposerait une dépendance inversée. Framing `u32` + JSON
   (`LengthDelimitedCodec`, cap 16 MiB : un worker fou ne fait pas allouer
   le hub sans borne), transport UDS/named pipes derrière un type unique,
   endpoint = chemin plateforme transmissible en argument CLI.
2. **`worker-echo` binaire dans `fluence-hub`** (pas dans `fluence-ipc`) :
   les kill-tests du hub le lancent via `CARGO_BIN_EXE_worker-echo`, qui
   n'existe que pour les binaires de la crate testée.
3. **Stack serveur : tokio + axum**, binaire mince (`main.rs` → `run()`),
   état partagé par `Arc<AppState>`, bus d'événements
   `tokio::sync::broadcast<ServerFrame>` filtré par topics/scope à l'envoi.
4. **Redaction P0 à deux étages** (PLAN 2.1) : (a) les valeurs P0 en
   mémoire hub sont des `secrecy::SecretString` — leur `Debug` n'expose
   jamais le contenu, l'accès est un `expose_secret()` explicite et
   greppable ; (b) une couche `tracing` de formatage redacte par **nom de
   champ** (denylist `draft`, `text`, `content`…) en ceinture de sécurité.
   Test rouge si une valeur marquée atteint la sortie.
5. **Store : rusqlite + `bundled-sqlcipher-vendored-openssl`** (zéro
   dépendance système, chiffrement AES-256 partout pareil), migrations
   refinery. **Acteur à file de commandes** (thread dédié + mpsc) : l'ordre
   des écritures est garanti — indispensable pour l'autosave du draft — et
   le code SQL reste synchrone et lisible. `journal_mode=WAL` +
   `synchronous=FULL` : le « ≤ 1 s de frappe perdue » de la SPEC couvre la
   coupure courant, pas seulement le kill du process ; au débit de
   l'autosave (≤ 2 écritures/s), le coût du fsync est invisible.
6. **Tokens** : 32 octets aléatoires (`flt_` + base64url) ; le store ne
   garde que le **SHA-256** (défense en profondeur dans une base déjà
   chiffrée). Fenêtre d'appairage en mémoire seulement : un redémarrage la
   ferme (comportement sûr par défaut).
7. **Routes ajoutées au registre** (chaîne anti-dérive, même flux
   qu'ADR-0004) : `POST /pair/window` (scope `system` — la SPEC exige une
   fenêtre « ouverte explicitement depuis l'UI principale » sans nommer la
   route) et `GET /sessions/{id}/draft` (la reprise de session §2.A doit
   pouvoir relire le draft restauré ; le kill-test aussi).
8. **Backoff superviseur** : exponentiel base 200 ms ×2, plafond 10 s,
   jitter ±25 % **injecté** (déterministe en test). La mort d'un child est
   détectée par `wait()` (immédiat sur crash franc, < 500 ms requis) ; le
   heartbeat Ping/Pong (1 s, timeout 3 s) attrape les blocages sans mort.

## Conséquences

- Les kill-tests s'écrivent contre le binaire réel (`CARGO_BIN_EXE_*`),
  sur les deux OS.
- Dette assumée : l'acteur store sérialise les écritures (un seul thread) —
  largement suffisant Phase 2 ; à re-profiler quand la mémoire (P2)
  multipliera les lectures.
- La liste de crates SPEC §2.B gagne `fluence-ipc` (signalé dans PLAN §5 ;
  pas une contradiction, un ajout dans l'esprit de la SPEC).
