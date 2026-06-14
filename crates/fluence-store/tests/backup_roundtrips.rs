// SPDX-License-Identifier: Apache-2.0

//! 7.3 — backup/restore is a *test*, not a promise (PLAN 7.3): a populated
//! store backs up under a recovery secret and restores onto a **different**
//! machine key with every byte intact — P0 drafts included. The wrong secret
//! never half-restores, and a backup never overwrites. Content purge erases P0
//! while keeping device pairings and the audit trail.

use fluence_protocol::api::pair::{DeviceKind, Scope};
use fluence_store::{
    KeySource, NewAccessEntry, NewDevice, RecoverySecret, Store, StoreConfig, StoreError, back_up,
    restore,
};
use secrecy::{ExposeSecret, SecretString};

/// The P0 draft we follow across backup/restore: if at-rest encryption ever
/// regressed, this string would appear in the archive bytes (asserted below).
const SECRET_DRAFT: &str = "contenu intime a sauver et restaurer";

async fn seed(store: &Store) {
    store
        .insert_device(NewDevice {
            device_id: "dev-1".into(),
            token_hash: [7u8; 32],
            name: "Tablette du lit".into(),
            kind: DeviceKind::Tablet,
            scope: Scope::Control,
        })
        .await
        .expect("device");
    store
        .upsert_draft("s1".into(), SecretString::from(SECRET_DRAFT), 9, 4242)
        .await
        .expect("draft");
    let profile: fluence_protocol::api::profiles::Profile =
        serde_json::from_str(r#"{"id":"default","name":"Claire"}"#).expect("profile");
    store.put_profile(profile).await.expect("profile");
    store
        .journal_append(NewAccessEntry {
            device_id: Some("dev-1".into()),
            action: "device.paired".into(),
            detail: None,
        })
        .await
        .expect("journal");
}

#[tokio::test]
async fn backup_restores_across_machine_keys_with_p0_intact() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store_path = dir.path().join("store.db");
    let key_a = KeySource::File(dir.path().join("a.key"));

    // A populated, then cleanly closed, source store (close checkpoints the
    // WAL so the .db file is self-contained for the file-level backup).
    let store = Store::open(StoreConfig {
        path: store_path.clone(),
        key: key_a.clone(),
    })
    .await
    .expect("open");
    seed(&store).await;
    store.close().await.expect("close");

    // Back up under a fresh recovery secret.
    let secret = RecoverySecret::generate();
    let archive = dir.path().join("fluence.backup");
    back_up(&store_path, &key_a, &archive, &secret).expect("back up");
    assert!(archive.exists(), "archive written");

    // The archive is itself encrypted: no plaintext P0, not a plain SQLite file.
    let raw = std::fs::read(&archive).expect("read archive");
    let needle = SECRET_DRAFT.as_bytes();
    assert!(
        !raw.windows(needle.len()).any(|w| w == needle),
        "archive leaks plaintext P0"
    );
    assert!(
        !raw.starts_with(b"SQLite format 3"),
        "archive is not encrypted"
    );

    // Restore onto a DIFFERENT machine key (the new-install scenario).
    let restored_path = dir.path().join("restored.db");
    let key_b = KeySource::File(dir.path().join("b.key"));
    restore(&archive, &secret, &restored_path, &key_b).expect("restore");

    // Everything survived, decrypting under the new key — P0 included.
    let restored = Store::open(StoreConfig {
        path: restored_path,
        key: key_b,
    })
    .await
    .expect("open restored");
    let draft = restored
        .draft("s1".into())
        .await
        .expect("read")
        .expect("present");
    assert_eq!(draft.text.expose_secret(), SECRET_DRAFT);
    assert_eq!(draft.caret, 9);
    assert_eq!(draft.updated_at_micros, 4242);
    let devices = restored.list_devices().await.expect("devices");
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].scope, Scope::Control);
    let profile = restored
        .profile("default".into())
        .await
        .expect("read")
        .expect("present");
    assert_eq!(profile.name, "Claire");
    let journal = restored.journal_recent(10).await.expect("journal");
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].action, "device.paired");
}

#[tokio::test]
async fn wrong_secret_never_restores() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store_path = dir.path().join("store.db");
    let key_a = KeySource::File(dir.path().join("a.key"));
    let store = Store::open(StoreConfig {
        path: store_path.clone(),
        key: key_a.clone(),
    })
    .await
    .expect("open");
    seed(&store).await;
    store.close().await.expect("close");

    let secret = RecoverySecret::generate();
    let archive = dir.path().join("fluence.backup");
    back_up(&store_path, &key_a, &archive, &secret).expect("back up");

    // A different secret must be rejected as a wrong key — never a partial or
    // silently-empty restore.
    let wrong = RecoverySecret::generate();
    let restored_path = dir.path().join("restored.db");
    let key_b = KeySource::File(dir.path().join("b.key"));
    let error = restore(&archive, &wrong, &restored_path, &key_b).expect_err("must refuse");
    assert!(matches!(error, StoreError::WrongKey), "got: {error}");
    assert!(
        !restored_path.exists(),
        "a failed restore leaves no partial store behind"
    );
}

#[tokio::test]
async fn backup_never_overwrites_an_existing_archive() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store_path = dir.path().join("store.db");
    let key_a = KeySource::File(dir.path().join("a.key"));
    let store = Store::open(StoreConfig {
        path: store_path.clone(),
        key: key_a.clone(),
    })
    .await
    .expect("open");
    seed(&store).await;
    store.close().await.expect("close");

    let secret = RecoverySecret::generate();
    let archive = dir.path().join("fluence.backup");
    std::fs::write(&archive, b"precious pre-existing file").expect("seed file");
    let error = back_up(&store_path, &key_a, &archive, &secret).expect_err("must refuse");
    assert!(matches!(error, StoreError::Key(_)), "got: {error}");
    // The pre-existing file is left untouched.
    assert_eq!(
        std::fs::read(&archive).expect("read"),
        b"precious pre-existing file"
    );
}

#[tokio::test]
async fn purge_content_erases_p0_but_keeps_devices_and_journal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = StoreConfig {
        path: dir.path().join("store.db"),
        key: KeySource::File(dir.path().join("store.key")),
    };
    let store = Store::open(config).await.expect("open");
    seed(&store).await;

    let removed = store.purge_content().await.expect("purge");
    assert_eq!(removed, 2, "one draft + one profile");
    assert!(store.draft("s1".into()).await.expect("read").is_none());
    assert!(
        store
            .profile("default".into())
            .await
            .expect("read")
            .is_none(),
        "profiles are personal content and must be erased"
    );
    // Device pairings and the audit trail survive a content purge.
    assert_eq!(store.list_devices().await.expect("devices").len(), 1);
    assert_eq!(store.journal_recent(10).await.expect("journal").len(), 1);
}
