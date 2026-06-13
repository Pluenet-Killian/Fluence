// SPDX-License-Identifier: Apache-2.0

//! T4 — the draft autosave buffer's correctness contract (SPEC §2.C
//! autosave, §9.A deletion). These drive `AppState` directly so the
//! buffer/delete/flush ordering is deterministic, with no process spawning.

use fluence_hub::config::HubConfig;
use fluence_hub::events::EventBus;
use fluence_hub::state::{AppState, PendingDraft};
use fluence_store::{KeySource, Store, StoreConfig};
use secrecy::{ExposeSecret, SecretString};

/// Builds an `AppState` backed by a real (temp, file-keyed) store.
async fn test_state(dir: &tempfile::TempDir) -> AppState {
    let config = HubConfig {
        data_dir: dir.path().to_owned(),
        store_key_file: Some(dir.path().join("store.key")),
        ..HubConfig::default()
    };
    let store = Store::open(StoreConfig {
        path: dir.path().join("store.db"),
        key: KeySource::File(dir.path().join("store.key")),
    })
    .await
    .expect("store opens");
    AppState::new(config, store, EventBus::new())
}

fn draft(text: &str, caret: u32) -> PendingDraft {
    PendingDraft {
        text: SecretString::from(text.to_owned()),
        caret,
        updated_at_micros: 1,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn flush_persists_a_buffered_draft() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(&dir).await;

    let _ = state.buffer_draft("s1".into(), draft("bonjour", 7));
    state.flush_drafts().await;

    let stored = state
        .store()
        .draft("s1".into())
        .await
        .expect("read")
        .expect("present");
    assert_eq!(stored.text.expose_secret(), "bonjour");
    assert_eq!(stored.caret, 7);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn delete_suppresses_a_never_flushed_draft() {
    // The F10 guarantee: a session closed before its draft was flushed
    // must never reach the encrypted store (SPEC §9.A).
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(&dir).await;

    let _ = state.buffer_draft("s1".into(), draft("contenu a oublier", 5));
    state.discard_pending_draft("s1");
    state.flush_drafts().await;

    assert!(
        state
            .store()
            .draft("s1".into())
            .await
            .expect("read")
            .is_none(),
        "a closed session's draft must not be persisted"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retyping_after_close_reopens_the_session() {
    // Buffering again after a close clears the tombstone: the new draft is
    // a fresh write and must persist.
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(&dir).await;

    let _ = state.buffer_draft("s1".into(), draft("premier jet", 3));
    state.discard_pending_draft("s1");
    let _ = state.buffer_draft("s1".into(), draft("nouveau jet", 4));
    state.flush_drafts().await;

    let stored = state
        .store()
        .draft("s1".into())
        .await
        .expect("read")
        .expect("present");
    assert_eq!(stored.text.expose_secret(), "nouveau jet");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_buffered_draft_survives_a_store_flush_error() {
    // F01: the draft must stay buffered (for a later retry) when the store
    // rejects the write — never drained-then-lost. A keystroke acknowledged
    // to the user must not vanish from both RAM and disk (D-2.6).
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(&dir).await;

    let _ = state.buffer_draft("s1".into(), draft("acquittee mais pas encore ecrite", 9));

    // Kill the store actor: every handle now errors on use.
    state.store().clone().close().await.expect("close");
    state.flush_drafts().await; // upsert fails — must not drop the draft

    assert!(
        state.pending_draft("s1").is_some(),
        "a draft lost on a store flush error (F01)"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn batch_flush_persists_every_session_in_one_pass() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = test_state(&dir).await;

    for i in 0..5 {
        let _ = state.buffer_draft(format!("s{i}"), draft(&format!("draft {i}"), i));
    }
    state.flush_drafts().await;

    for i in 0..5 {
        let stored = state
            .store()
            .draft(format!("s{i}"))
            .await
            .expect("read")
            .expect("present");
        assert_eq!(stored.text.expose_secret(), format!("draft {i}"));
    }
}
