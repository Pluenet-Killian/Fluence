// SPDX-License-Identifier: Apache-2.0

//! T1 — the store keeps its promises: data survives reopen (the
//! kill-test's foundation), the wrong key never half-opens a database,
//! device auth lookups exclude revoked tokens, the journal stays P0-free
//! by shape.

use fluence_protocol::api::pair::{DeviceKind, Scope};
use fluence_store::{KeySource, NewAccessEntry, NewDevice, Store, StoreConfig, StoreError};
use secrecy::{ExposeSecret, SecretString};

fn config_in(dir: &tempfile::TempDir) -> StoreConfig {
    StoreConfig {
        path: dir.path().join("store.db"),
        key: KeySource::File(dir.path().join("store.key")),
    }
}

#[tokio::test]
async fn draft_survives_reopen() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = config_in(&dir);

    let store = Store::open(config.clone()).await.expect("open");
    store
        .upsert_draft(
            "s1".into(),
            SecretString::from("bonjour je voudr"),
            16,
            123_456,
        )
        .await
        .expect("upsert");
    store.close().await.expect("close");

    let reopened = Store::open(config).await.expect("reopen");
    let draft = reopened
        .draft("s1".into())
        .await
        .expect("read")
        .expect("present");
    assert_eq!(draft.text.expose_secret(), "bonjour je voudr");
    assert_eq!(draft.caret, 16);
    assert_eq!(draft.updated_at_micros, 123_456);
}

#[tokio::test]
async fn upsert_keeps_only_the_latest_draft() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(config_in(&dir)).await.expect("open");

    store
        .upsert_draft("s1".into(), SecretString::from("a"), 1, 10)
        .await
        .expect("first");
    store
        .upsert_draft("s1".into(), SecretString::from("ab"), 2, 20)
        .await
        .expect("second");

    let draft = store
        .draft("s1".into())
        .await
        .expect("read")
        .expect("present");
    assert_eq!(draft.text.expose_secret(), "ab");
    assert_eq!(draft.updated_at_micros, 20);
}

#[tokio::test]
async fn batch_upsert_persists_every_draft_in_one_transaction() {
    // F20: the autosave flush sends the whole tick as one batch (one fsync
    // for all of it) so its duration cannot grow with the session count and
    // blow the «&nbsp;≤ 1 s lost&nbsp;» bound (D-2.6). Every draft must
    // still land, and survive a reopen.
    let dir = tempfile::tempdir().expect("tempdir");
    let config = config_in(&dir);
    let store = Store::open(config.clone()).await.expect("open");

    let writes: Vec<fluence_store::DraftWrite> = (0..500)
        .map(|i| fluence_store::DraftWrite {
            session_id: format!("s{i}"),
            text: SecretString::from(format!("draft {i}")),
            caret: i,
            updated_at_micros: u64::from(i),
        })
        .collect();
    store.upsert_drafts(writes).await.expect("batch upsert");
    store.close().await.expect("close");

    let reopened = Store::open(config).await.expect("reopen");
    for i in [0u32, 1, 250, 499] {
        let draft = reopened
            .draft(format!("s{i}"))
            .await
            .expect("read")
            .unwrap_or_else(|| panic!("draft s{i} present"));
        assert_eq!(draft.text.expose_secret(), format!("draft {i}"));
        assert_eq!(draft.caret, i);
    }
}

#[tokio::test]
async fn batch_upsert_overwrites_and_keeps_latest_per_session() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(config_in(&dir)).await.expect("open");

    store
        .upsert_draft("s1".into(), SecretString::from("old"), 3, 10)
        .await
        .expect("seed");
    // A later batch carrying the same session must win (ON CONFLICT path
    // inside the transaction).
    store
        .upsert_drafts(vec![fluence_store::DraftWrite {
            session_id: "s1".into(),
            text: SecretString::from("new"),
            caret: 3,
            updated_at_micros: 20,
        }])
        .await
        .expect("batch");

    let draft = store
        .draft("s1".into())
        .await
        .expect("read")
        .expect("present");
    assert_eq!(draft.text.expose_secret(), "new");
    assert_eq!(draft.updated_at_micros, 20);
}

#[tokio::test]
async fn empty_batch_upsert_is_a_noop() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(config_in(&dir)).await.expect("open");
    store
        .upsert_drafts(Vec::new())
        .await
        .expect("empty batch ok");
}

#[tokio::test]
async fn wrong_key_is_a_clean_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = config_in(&dir);

    let store = Store::open(config.clone()).await.expect("open");
    store
        .upsert_draft("s1".into(), SecretString::from("secret"), 6, 1)
        .await
        .expect("write");
    store.close().await.expect("close");

    // Replace the key file: the database must refuse to open, never
    // half-open or silently recreate.
    std::fs::write(dir.path().join("store.key"), "0".repeat(64)).expect("corrupt key");
    let error = Store::open(config).await.expect_err("must refuse");
    assert!(matches!(error, StoreError::WrongKey), "got: {error}");
}

#[tokio::test]
async fn auth_lookup_excludes_revoked_devices() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(config_in(&dir)).await.expect("open");

    let hash = [7u8; 32];
    store
        .insert_device(NewDevice {
            device_id: "dev-1".into(),
            token_hash: hash,
            name: "Tablette du lit".into(),
            kind: DeviceKind::Tablet,
            scope: Scope::Control,
        })
        .await
        .expect("insert");

    let found = store.device_by_token_hash(hash).await.expect("lookup");
    assert_eq!(found.expect("present").scope, Scope::Control);

    store.revoke_device("dev-1".into()).await.expect("revoke");
    let after = store.device_by_token_hash(hash).await.expect("lookup");
    assert!(after.is_none(), "revoked tokens must not authenticate");

    // The caregiver view still lists it, with its revocation time.
    let all = store.list_devices().await.expect("list");
    assert_eq!(all.len(), 1);
    assert!(all[0].revoked_at.is_some());
}

#[tokio::test]
async fn journal_orders_newest_first_and_carries_no_content_field() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(config_in(&dir)).await.expect("open");

    for action in ["pair.window_opened", "device.paired", "auth.rejected"] {
        store
            .journal_append(NewAccessEntry {
                device_id: None,
                action: action.into(),
                detail: Some("kind=tablet".into()),
            })
            .await
            .expect("append");
    }

    let recent = store.journal_recent(2).await.expect("recent");
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].action, "auth.rejected");
    assert_eq!(recent[1].action, "device.paired");
}

#[tokio::test]
async fn journal_is_bounded_under_a_flood() {
    // F26: an unauthenticated loopback client can hammer `auth.rejected`.
    // The journal must stay bounded so it neither fills the home disk nor
    // floods the store connection the draft flusher shares (D-2.6). We
    // assert the row cap holds and the *newest* entries survive eviction.
    let dir = tempfile::tempdir().expect("tempdir");
    let config = config_in(&dir);
    let store = Store::open(config.clone()).await.expect("open");

    // Far more appends than the cap: every excess row must be evicted.
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

    // Reading the whole budget back returns at most the cap, and the very
    // first entry is the freshest write (newest-first ordering).
    let recent = store.journal_recent(10_000).await.expect("recent");
    assert!(
        recent.len() <= 5_000,
        "journal must stay bounded, got {} rows",
        recent.len()
    );
    assert_eq!(
        recent[0].detail.as_deref(),
        Some(format!("seq={}", total - 1).as_str()),
        "the newest entry must survive eviction"
    );

    // The bound holds across reopen (AUTOINCREMENT high-water mark).
    store.close().await.expect("close");
    let reopened = Store::open(config).await.expect("reopen");
    let after = reopened.journal_recent(10_000).await.expect("recent");
    assert!(after.len() <= 5_000, "bound must hold after reopen");
}

#[tokio::test]
async fn stale_draft_purge_removes_only_aged_rows() {
    // F09 disk bound: drafts have no natural expiry (deleted only on an
    // explicit session close), so a Control device looping PUTs under fresh
    // ids would grow the table without bound. The TTL purge reclaims rows
    // untouched past the cutoff, and *only* those — a live draft (touched
    // after the cutoff) must survive.
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(config_in(&dir)).await.expect("open");

    // Two drafts straddling the cutoff: one aged (1 000), one fresh (3 000).
    store
        .upsert_draft(
            "old".into(),
            SecretString::from("contenu a oublier"),
            0,
            1_000,
        )
        .await
        .expect("old");
    store
        .upsert_draft(
            "fresh".into(),
            SecretString::from("contenu intime"),
            0,
            3_000,
        )
        .await
        .expect("fresh");

    let purged = store.purge_stale_drafts(2_000).await.expect("purge");

    assert_eq!(purged, 1, "exactly the aged draft is purged");
    assert!(
        store.draft("old".into()).await.expect("read").is_none(),
        "an aged draft must be reclaimed (F09)"
    );
    assert!(
        store.draft("fresh".into()).await.expect("read").is_some(),
        "a draft touched after the cutoff must survive the purge"
    );
}

#[tokio::test]
async fn profiles_round_trip_through_contract_json() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(config_in(&dir)).await.expect("open");

    let profile: fluence_protocol::api::profiles::Profile =
        serde_json::from_str(r#"{"id":"default","name":"Claire"}"#).expect("profile");
    store.put_profile(profile.clone()).await.expect("put");
    let back = store
        .profile("default".into())
        .await
        .expect("get")
        .expect("present");
    assert_eq!(back, profile);
    assert!(store.profile("absent".into()).await.expect("get").is_none());
}

#[tokio::test]
async fn database_file_is_actually_encrypted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(config_in(&dir)).await.expect("open");
    store
        .upsert_draft(
            "s1".into(),
            SecretString::from("contenu intime lisible"),
            5,
            1,
        )
        .await
        .expect("write");
    store.close().await.expect("close");

    let raw = std::fs::read(dir.path().join("store.db")).expect("read file");
    let needle = b"contenu intime lisible";
    let leaked = raw.windows(needle.len()).any(|window| window == needle);
    assert!(!leaked, "plaintext P0 found in the database file");
    // A plain SQLite file starts with this magic; an encrypted one must not.
    assert!(
        !raw.starts_with(b"SQLite format 3"),
        "file is not encrypted"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_a_store_joins_its_owning_thread() {
    // Regression: the owning thread used to be detached, so on a graceful drop
    // (no `close`) its final WAL checkpoint — touching SQLCipher/OpenSSL —
    // raced process/runtime teardown, an intermittent SIGABRT. Drop now joins
    // the thread, so by the time `drop` returns the checkpoint has run and the
    // connection has closed: SQLite has truncated/removed the -wal sidecar.
    // This is deterministic only if the join actually happens.
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::open(config_in(&dir)).await.expect("open");
    store
        .upsert_draft("s".into(), SecretString::from("contenu"), 1, 1)
        .await
        .expect("write");
    drop(store); // No close(): rely on Drop joining the thread.

    let wal = dir.path().join("store.db-wal");
    let wal_len = std::fs::metadata(&wal).map_or(0, |m| m.len());
    assert_eq!(
        wal_len, 0,
        "a joined drop must have checkpointed and closed the connection (WAL drained)"
    );
}
