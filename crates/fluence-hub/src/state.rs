// SPDX-License-Identifier: Apache-2.0

//! Shared hub state: store handle, event bus, pairing window, draft
//! autosave buffer, supervised workers.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use fluence_protocol::api::pair::Scope;
use fluence_store::{DraftWrite, Store};
use secrecy::SecretString;
use tokio::time::Instant;

use crate::config::HubConfig;
use crate::events::EventBus;
use crate::supervisor::WorkerHandle;

/// Pairing window lifetime (SPEC §2.A: 2 minutes).
pub const PAIRING_WINDOW_TTL: Duration = Duration::from_secs(120);
/// Failed attempts that burn the window (SPEC §2.A: rate-limited).
pub const PAIRING_MAX_ATTEMPTS: u32 = 5;
/// Draft flush period — a throttle, not a strict debounce: under
/// continuous typing a pure debounce would never flush and break the
/// «&nbsp;≤ 1 s lost&nbsp;» guarantee (D-2.6). Worst-case loss =
/// period + commit time ≪ 1 s.
pub const DRAFT_FLUSH_PERIOD: Duration = Duration::from_millis(500);

/// Locks a state mutex, tolerating poisoning.
///
/// A poisoned lock means a thread panicked mid-update. Every mutex here
/// guards a plain data holder with no cross-field invariant, so recovering
/// the data keeps the keyboard path alive (D-2.6) instead of letting one
/// panic cascade into the whole hub.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// An open pairing window.
#[derive(Debug)]
pub struct PairingWindow {
    /// Eight-digit single-use code.
    pub code: String,
    /// When the window closes.
    pub expires_at: Instant,
    /// Wall-clock expiry (returned to the opener).
    pub expires_at_utc: DateTime<Utc>,
    /// Failed attempts so far.
    pub attempts: u32,
    /// Scope this window grants.
    pub scope: Scope,
}

/// A draft not yet persisted (P0 — text behind `SecretString`).
pub struct PendingDraft {
    /// Draft text.
    pub text: SecretString,
    /// Caret position.
    pub caret: u32,
    /// Client keystroke timestamp (µs) — the loss-bound witness.
    pub updated_at_micros: u64,
}

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

/// Coordination state shared between writers (HTTP handlers) and the
/// flusher, guarded by one mutex so every decision is serialized.
///
/// `dirty` holds the latest unflushed draft per session; `deleted_at`
/// records, per session, the generation that was current when the session
/// was last closed. A flush of generation `g` is suppressed whenever the
/// session has a tombstone `t >= g` — i.e. a delete that observed that
/// draft (or a newer one) must win, never the in-flight upsert.
struct DraftBuffer {
    dirty: HashMap<String, BufferedDraft>,
    /// Tombstones: highest generation seen by a `delete_session`, per
    /// closed session. Reclaimed once the session holds no dirty draft
    /// (see [`prune_orphan_tombstones_locked`]) — bounded by the live
    /// working set, not by history.
    deleted_at: HashMap<String, u64>,
}

/// Drops tombstones for sessions that no longer have a buffered draft: with
/// nothing in `dirty`, no upsert can be in flight, so the tombstone is
/// spent. Split-borrows the two maps so `retain` can consult `dirty`.
fn prune_orphan_tombstones_locked(drafts: &mut DraftBuffer) {
    let DraftBuffer { dirty, deleted_at } = drafts;
    deleted_at.retain(|session_id, _| dirty.contains_key(session_id));
}

struct Inner {
    config: HubConfig,
    store: Store,
    bus: EventBus,
    started_at: DateTime<Utc>,
    pairing: Mutex<Option<PairingWindow>>,
    drafts: Mutex<DraftBuffer>,
    /// Monotonic source for draft generations (never wraps in practice:
    /// ≤ 2 writes/s for centuries).
    draft_generation: AtomicU64,
    workers: Mutex<Vec<Arc<WorkerHandle>>>,
}

/// Cheaply cloneable hub state.
#[derive(Clone)]
pub struct AppState(Arc<Inner>);

impl AppState {
    /// Assembles the state. Workers register afterwards
    /// ([`AppState::register_worker`]).
    #[must_use]
    pub fn new(config: HubConfig, store: Store, bus: EventBus) -> Self {
        Self(Arc::new(Inner {
            config,
            store,
            bus,
            started_at: Utc::now(),
            pairing: Mutex::new(None),
            drafts: Mutex::new(DraftBuffer {
                dirty: HashMap::new(),
                deleted_at: HashMap::new(),
            }),
            draft_generation: AtomicU64::new(0),
            workers: Mutex::new(Vec::new()),
        }))
    }

    /// Hub configuration.
    #[must_use]
    pub fn config(&self) -> &HubConfig {
        &self.0.config
    }

    /// Encrypted store handle.
    #[must_use]
    pub fn store(&self) -> &Store {
        &self.0.store
    }

    /// System event bus.
    #[must_use]
    pub fn bus(&self) -> &EventBus {
        &self.0.bus
    }

    /// Hub start time (health).
    #[must_use]
    pub fn started_at(&self) -> DateTime<Utc> {
        self.0.started_at
    }

    /// Locked access to the pairing window slot (held briefly for an
    /// atomic open/consume decision).
    pub fn pairing(&self) -> std::sync::MutexGuard<'_, Option<PairingWindow>> {
        lock(&self.0.pairing)
    }

    /// Registers a supervised worker (boot time).
    pub fn register_worker(&self, handle: Arc<WorkerHandle>) {
        lock(&self.0.workers).push(handle);
    }

    /// Snapshot of supervised worker handles.
    #[must_use]
    pub fn workers(&self) -> Vec<Arc<WorkerHandle>> {
        lock(&self.0.workers).clone()
    }

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

    /// Takes the pending draft of one session, if any (freshest read).
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

    /// Snapshots the drafts to persist this tick **without** emptying the
    /// buffer (F01): a draft only leaves RAM once the store *confirms* its
    /// write ([`AppState::flush_drafts`]). Returns two aligned vectors —
    /// `keys` of `(session_id, generation)` for the post-flush removal, and
    /// the `DraftWrite`s for the store (P0 text moved, not re-cloned).
    ///
    /// A draft whose session was closed since it was buffered
    /// (`tombstone >= generation`) is skipped — the `DELETE` wins, so a
    /// freshly closed conversation is never written back (F10 / SPEC §9.A).
    /// The filter runs under the lock immediately before the store call, so
    /// the suppression window is as tight as the async boundary allows.
    fn snapshot_survivors(&self) -> (Vec<(String, u64)>, Vec<DraftWrite>) {
        let drafts = lock(&self.0.drafts);
        let mut keys = Vec::new();
        let mut writes = Vec::new();
        for (session_id, buffered) in &drafts.dirty {
            let suppressed = drafts
                .deleted_at
                .get(session_id)
                .is_some_and(|&tombstone| tombstone >= buffered.generation);
            if suppressed {
                continue;
            }
            keys.push((session_id.clone(), buffered.generation));
            writes.push(DraftWrite {
                session_id: session_id.clone(),
                text: buffered.draft.text.clone(),
                caret: buffered.draft.caret,
                updated_at_micros: buffered.draft.updated_at_micros,
            });
        }
        (keys, writes)
    }

    /// Discards the unflushed draft of a closing session and plants a
    /// tombstone so a concurrently draining flusher cannot resurrect it.
    ///
    /// The tombstone records the generation current at delete time; the
    /// flusher (which may already hold a snapshot of this draft) re-checks
    /// it under the same lock right before persisting (see
    /// [`AppState::flush_drafts`]). This makes a `DELETE` authoritative
    /// against the autosave loop — a closed conversation's P0 never outlives
    /// it (SPEC §9.A; closes the delete-vs-flush race F10).
    pub fn discard_pending_draft(&self, session_id: &str) {
        let generation = self.0.draft_generation.fetch_add(1, Ordering::Relaxed);
        let mut drafts = lock(&self.0.drafts);
        drafts.dirty.remove(session_id);
        // Keep the highest generation if a previous (unflushed) tombstone
        // exists — never weaken an existing delete decision.
        let slot = drafts.deleted_at.entry(session_id.to_owned()).or_insert(0);
        *slot = (*slot).max(generation);
    }

    /// Flushes the dirty drafts to the store in one transaction (one fsync
    /// for the whole batch). Called by the periodic flusher and by graceful
    /// shutdown.
    ///
    /// **Durability (F01 / D-2.6).** A draft is removed from the buffer
    /// *only after* the store acknowledges its write, and only if no fresher
    /// keystroke arrived meanwhile (generation unchanged). On a store error
    /// the whole batch stays buffered and is retried next tick — an
    /// acknowledged keystroke is never lost from both RAM and disk.
    ///
    /// **Deletion (F10 / SPEC §9.A).** A draft whose session was closed
    /// since it was buffered is excluded from the batch (the `DELETE` wins),
    /// decided under the buffer lock — never an `.await` while holding it.
    /// The narrow residual race (a delete landing after the snapshot) is
    /// closed by `delete_session` also issuing a store `delete_draft`.
    pub async fn flush_drafts(&self) {
        let (keys, writes) = self.snapshot_survivors();
        if writes.is_empty() {
            // Nothing to write, but stale tombstones may remain; reclaim them.
            self.prune_orphan_tombstones();
            return;
        }

        match self.store().upsert_drafts(writes).await {
            Ok(()) => {
                let mut drafts = lock(&self.0.drafts);
                for (session_id, generation) in &keys {
                    // Drop the entry only if it is still the exact version we
                    // persisted: a newer keystroke (higher generation) or a
                    // delete (removed from `dirty`) must survive untouched.
                    if drafts
                        .dirty
                        .get(session_id)
                        .is_some_and(|buffered| buffered.generation == *generation)
                    {
                        drafts.dirty.remove(session_id);
                    }
                }
                prune_orphan_tombstones_locked(&mut drafts);
            }
            Err(error) => {
                // Transient store failure (e.g. disk full): keep every draft
                // buffered for the next tick. `StoreError` carries no P0.
                tracing::error!(%error, "draft flush failed; drafts kept buffered for retry");
            }
        }
    }

    /// Reclaims tombstones whose session no longer has a buffered draft:
    /// nothing buffered means no upsert can be in flight, so the tombstone
    /// has done its job. Keeps `deleted_at` bounded by the live working set.
    fn prune_orphan_tombstones(&self) {
        prune_orphan_tombstones_locked(&mut lock(&self.0.drafts));
    }

    /// Appends an access-journal entry (never P0 in `detail`).
    pub async fn journal(&self, action: &str, device_id: Option<String>, detail: Option<&str>) {
        let entry = fluence_store::NewAccessEntry {
            device_id,
            action: action.to_owned(),
            detail: detail.map(ToOwned::to_owned),
        };
        if let Err(error) = self.store().journal_append(entry).await {
            tracing::error!(%error, "journal append failed");
        }
    }

    /// Spawns the periodic draft flusher (runs until the hub stops).
    pub fn spawn_draft_flusher(&self) {
        let state = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(DRAFT_FLUSH_PERIOD);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                state.flush_drafts().await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use fluence_store::{KeySource, Store, StoreConfig};
    use secrecy::ExposeSecret;

    use super::*;
    use crate::config::HubConfig;
    use crate::events::EventBus;

    async fn test_state(dir: &tempfile::TempDir) -> AppState {
        let store = Store::open(StoreConfig {
            path: dir.path().join("store.db"),
            key: KeySource::File(dir.path().join("store.key")),
        })
        .await
        .expect("store opens");
        AppState::new(HubConfig::default(), store, EventBus::new())
    }

    fn pending(text: &str) -> PendingDraft {
        PendingDraft {
            text: SecretString::from(text.to_owned()),
            caret: 0,
            updated_at_micros: 1,
        }
    }

    /// F10: a session closed before the flush snapshots it is never a flush
    /// survivor, so its P0 is never written to the encrypted store
    /// (SPEC §9.A). `discard_pending_draft` removes the dirty entry and
    /// plants a tombstone; `snapshot_survivors` then excludes it, and a full
    /// `flush_drafts` writes nothing for it.
    #[tokio::test]
    async fn a_closed_session_is_never_a_flush_survivor() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir).await;
        let session = "s-race".to_owned();

        state.buffer_draft(session.clone(), pending("p0 intime"));
        // The DELETE lands before the flush takes its snapshot.
        state.discard_pending_draft(&session);

        let (keys, writes) = state.snapshot_survivors();
        assert!(
            keys.is_empty() && writes.is_empty(),
            "a closed session must not survive"
        );

        state.flush_drafts().await;
        assert!(
            state.store().draft(session).await.expect("read").is_none(),
            "a deleted session's P0 was resurrected (F10)"
        );
    }

    /// F01: a transient store error must not lose a buffered draft — it
    /// stays in RAM for the next tick. Here the store is closed mid-life so
    /// every write errors; the draft must remain buffered.
    #[tokio::test]
    async fn a_store_error_keeps_the_draft_buffered() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir).await;
        state.buffer_draft("s1".to_owned(), pending("pas encore ecrite"));

        state.store().clone().close().await.expect("close");
        state.flush_drafts().await; // every write errors

        assert!(
            state.pending_draft("s1").is_some(),
            "the draft must stay buffered after a store error (F01)"
        );
    }

    /// Typing again into a previously closed session reopens it: the new
    /// draft clears the tombstone and flushes normally.
    #[tokio::test]
    async fn retyping_after_delete_clears_the_tombstone_and_persists() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir).await;
        let session = "s-reopen".to_owned();

        state.buffer_draft(session.clone(), pending("first"));
        state.discard_pending_draft(&session);
        // A fresh keystroke after the close — strictly newer generation.
        state.buffer_draft(session.clone(), pending("second"));

        state.flush_drafts().await;

        let restored = state
            .store()
            .draft(session)
            .await
            .expect("read")
            .expect("the reopened draft is persisted");
        assert_eq!(restored.text.expose_secret(), "second");
    }

    /// The common (non-racing) case still persists drafts.
    #[tokio::test]
    async fn flush_persists_a_live_draft() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir).await;
        let session = "s-live".to_owned();

        state.buffer_draft(session.clone(), pending("bonjour"));
        state.flush_drafts().await;

        let restored = state
            .store()
            .draft(session)
            .await
            .expect("read")
            .expect("present");
        assert_eq!(restored.text.expose_secret(), "bonjour");
    }
}
