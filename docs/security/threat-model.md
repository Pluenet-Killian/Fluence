<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# Threat model — checklist vérifiable (SPEC §9.A, D-9.1/D-9.3)

Ce document décline le threat model résumé de la SPEC §9.A **point par point** :
pour chaque menace, la mitigation concrète (fichier) **et le test** qui la tient.
C'est la passe sécurité interne de la Phase 7.7 ; il est tenu à jour à chaque
changement de surface (« threat model publié et à jour », D-9.3).

Principe directeur : Fluence traite des données **P0 intime** (conversations,
mémoire, voix) pour des personnes dont c'est parfois le seul canal de parole. Une
faille peut exposer ce qu'on a de plus privé **ou** couper la parole. Deux
propriétés priment donc à parts égales : **confidentialité** des P0 et
**disponibilité** du clavier (« le clavier parle toujours », SPEC §2.C).

## Classes de données (SPEC §9.A)

| Classe | Exemples | Traitement attendu |
|---|---|---|
| **P0 intime** | conversations, mémoire, voix, brouillons | chiffré au repos, ne quitte jamais le foyer, **jamais dans les logs** |
| **P1 personnel** | profils, calibrations, config | chiffré au repos, exportable |
| **P2 technique** | latences, état workers | local ; agrégats anonymes opt-in (off par défaut) |

## Menaces couvertes

### 1. Voleur du PC — chiffrement au repos

- **Scénario** : un tiers s'empare du disque/de la machine et lit les fichiers.
- **Mitigation** : tout le store est chiffré SQLCipher (AES-256) ;
  `crates/fluence-store/src/actor.rs` applique la clé brute en tout premier ;
  la clé maîtresse vit dans le keystore OS (DPAPI Windows) ou un fichier 0600
  (`crates/fluence-store/src/key.rs`), jamais en clair à côté de la base —
  l'avertissement F06 le signale au boot (`crates/fluence-hub/src/lib.rs`,
  `warn_if_at_rest_degraded`). Les **sauvegardes** sont elles aussi des bases
  SQLCipher indépendantes, re-chiffrées sous le secret de récupération
  (`crates/fluence-store/src/backup.rs`).
- **Tests** : `database_file_is_actually_encrypted`,
  `wrong_key_is_a_clean_error` (`tests/store_roundtrips.rs`) ;
  `backup_restores_across_machine_keys_with_p0_intact` (vérifie aussi que
  **l'archive** ne contient aucun P0 en clair, `tests/backup_roundtrips.rs`).
- **Limite documentée (ADR-0005)** : sur Linux headless (pas de Secret Service),
  la clé est un fichier 0600 — protection moindre contre le vol de disque, à
  renforcer (TPM/keyutils) ultérieurement.

### 2. Site web malveillant — drive-by sur le port local

- **Scénario** : la victime visite une page web qui tente d'appeler
  `http://127.0.0.1:7411` (l'API du hub) depuis le navigateur.
- **Mitigation** : CORS en **liste blanche vide** (le composeur est same-origin),
  donc toute requête cross-origin est refusée ; et **toute** route hors `/pair*`
  exige le token `X-Fluence-Token` qu'une page tierce ne possède pas
  (`crates/fluence-hub/src/api/mod.rs`, `crates/fluence-hub/src/auth.rs`).
- **Tests** : `cross_origin_browser_calls_are_refused`,
  `tokenless_requests_get_a_uniform_401`, `pair_info_is_the_only_anonymous_read`
  (`tests/api_security.rs`).

### 3. Curieux du LAN — tokens à scopes (TLS en mode foyer)

- **Scénario** : un appareil du réseau local sonde le hub.
- **Mitigation** : écoute en **loopback par défaut** (`config.rs`,
  `listen_addr = 127.0.0.1`) ; l'appairage délivre des tokens **à scopes**
  (`display`/`control`/`care`/`system`) dont seul le hash SHA-256 est stocké
  (`auth.rs`, `fluence-store`). Le code d'appairage est à usage unique, comparé
  en **temps constant**, et une rafale d'essais brûle la fenêtre (anti-brute-force).
- **Tests** : `scope_route_matrix_is_enforced`,
  `pairing_codes_are_single_use_and_brute_force_burns_the_window`
  (`tests/api_security.rs`).
- **Dette assumée (#10)** : le mode foyer (LAN + **TLS** + mDNS) est explicitement
  opt-in et non encore livré ; tant qu'il ne l'est pas, exposer le hub au LAN
  n'est pas supporté. Documenté, pas masqué.

### 4. Aidant outrepassant ses droits — scopes + journal + révocation

- **Scénario** : un proche/aidant légitime tente d'accéder au-delà de son rôle
  (lire les conversations P0 depuis l'espace aidant).
- **Mitigation** : séparation de scopes — l'espace aidant a le scope `care`
  (santé, journal, appareils) et **n'atteint pas** le contenu P0, réservé au
  scope `control` (`api/mod.rs` : `/system/journal` est `care`, le contenu de
  session est `control`). Toute action sensible est tracée au **journal d'accès**
  (métadonnées seulement — jamais de P0), et un appareil se **révoque**
  (`Store::revoke_device` ; endpoint aidant Phase 7.2).
- **Tests** : `scope_route_matrix_is_enforced` (`tests/api_security.rs`) ;
  `auth_lookup_excludes_revoked_devices`,
  `journal_orders_newest_first_and_carries_no_content_field`
  (`tests/store_roundtrips.rs`).
- **Legacy access** (déchiffrement posthume par un proche désigné) : **opt-in
  explicite, défaut NON** (la voix de quelqu'un ne s'hérite pas en silence) —
  reposera sur le kit de secours (SPEC §9.A).

## Cycle de vie des données P0

| Contrôle | Mécanisme | Test |
|---|---|---|
| Jamais de P0 dans les logs | rapports/journal expurgés par construction | `hub_logs_never_contain_draft_content` (`tests/kill_tests.rs`) |
| Journal = métadonnées seules | pas de champ contenu, borné à 5 000 lignes | `journal_..._carries_no_content_field`, `journal_is_bounded_under_a_flood` |
| Suppression réelle (oubli) | `Store::purge_content` efface brouillons+profils puis `VACUUM` | `purge_content_erases_p0_but_keeps_devices_and_journal` |
| Sauvegarde restaurable | export chiffré ; **restauration testée en CI** | `tests/backup_roundtrips.rs` |
| Disque borné (DoS) | limite de corps de requête ; purge TTL des brouillons | `an_oversized_draft_is_refused_and_never_buffered`, `stale_draft_purge_removes_only_aged_rows` |

## Disponibilité — « le clavier parle toujours » (SPEC §2.C)

La parole est vitale : composer et vocaliser ne dépendent **jamais** de la santé
des composants IA (workers supervisés en processus enfants, dégradation explicite).
Un déni de service visant l'IA ne doit pas casser le clavier.

- **Mitigation** : superviseur watchdog (`crates/fluence-hub/src/supervisor/`) ;
  repli n-gram quand le LLM est absent ; voix OS toujours derrière Piper.
- **Tests** : suite `tests/kill_tests.rs` (kill-tests) ; `tests/next_chars.rs`
  (dégradation vers le n-gram, jamais de 5xx).

## Robustesse des entrées non fiables (parseurs)

- **Math d'entrée** : les coordonnées, confiances et pose tête arrivent d'un
  *client* (frames `ptr`/pose). JSON ne porte pas `NaN` mais porte ±∞ et des
  magnitudes extrêmes. La math hub-side (One Euro, fusion, magnétisme,
  head-affine) ne **panique jamais** et préserve la finitude — au pire une coord
  hostile rate sa cible. Verrouillé par property-tests adverses
  (`crates/fluence-input/tests/robustness.rs`), cross-OS, à chaque build CI.
- **Framing IPC** : longueur préfixée **bornée à 16 MiB** (`LengthDelimitedCodec`,
  `crates/fluence-ipc/src/transport.rs`) — un pair fautif ne peut pas faire
  allouer sans borne. Messages = serde (pas de parseur écrit à la main).
- **Calibration** : `Calibrator::fit` **rejette** toute feature non finie
  (durcissement audit adversarial, `crates/fluence-input/src/calibration.rs`).
- **Dépendances** : `cargo deny` (advisories RustSec) bloquant en CI + nightly.

## Hors périmètre (documenté — SPEC §9.A)

Assumés non couverts, et c'est un choix explicite, pas un oubli :

- **Administrateur OS malveillant** : qui contrôle l'OS contourne tout keystore.
- **Attaquant physique persistant** : accès matériel répété (cold boot, keylogger
  matériel) hors scope d'un logiciel local.
- **Acteurs étatiques** : hors modèle de menace d'un outil communautaire.
- **Mémoire vive** : pas de zeroization systématique (hors scope) — mais aucun P0
  dans les dumps de crash (rapports expurgés par construction).

## Dette de sécurité suivie

- **Mode foyer TLS + mDNS** (#10) — tant qu'absent, LAN non supporté.
- **Clé Linux headless** en fichier 0600 (ADR-0005) — TPM/keyutils plus tard.
- **Signatures minisign** des manifestes de modèles : vérification livrée
  (Phase 7.4) ; la **clé privée de release** est un secret d'opérateur (gate).
- **Fuzzing continu** (`cargo-fuzz`) en nightly : property-tests livrés ;
  cibles libFuzzer à ajouter (D-9.3).
- **Audit professionnel** : dette assumée jusqu'à ce que des moyens existent
  (trajectoire D-12.3) ; revue communautaire avant bêta publique.
