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
