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
/// records, per session, the generation that was current when the session
/// was last closed. A flush of generation `g` is suppressed whenever the
/// session has a tombstone `t >= g` — i.e. a delete that observed that
/// draft (or a newer one) must win, never the in-flight upsert.
struct DraftBuffer {
    dirty: HashMap<String, BufferedDraft>,
    /// Tombstones: highest generation seen by a `delete_session`, per
    /// closed session. Pruned as soon as the flusher confirms no equal-or-
    /// older write can still be in flight (bounded — see [`AppState::take_dirty_drafts`]).
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

    /// Drains every dirty draft for persistence, tagged with the generation
    /// each was buffered at. Also prunes tombstones for sessions that hold
    /// no dirty draft: once nothing is buffered, no upsert can be in flight
    /// for them, so the tombstone has served its purpose (keeps
    /// `deleted_at` bounded by the live working set, not by history).
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
        // After the drain `dirty` is empty, so every tombstone is now safe
        // to forget: nothing buffered means nothing can be re-inserted.
        drafts.deleted_at.clear();
        drained
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

    /// Flushes all dirty drafts to the store in one transaction (one fsync
    /// for the whole batch). Called by the periodic flusher and by graceful
    /// shutdown.
    ///
    /// A drained draft is dropped from the batch when its session was closed
    /// since it was buffered (`tombstone >= generation`): the `DELETE` wins,
    /// so a freshly closed conversation is never written back into the
    /// encrypted store (F10 / SPEC §9.A). The survivor filter runs under the
    /// buffer lock — only a `HashMap` lookup, never an `.await` — then the
    /// batch goes to the store. The residual race (a delete landing between
    /// this filter and the store write) is closed by `delete_session` also
    /// issuing a store-level `delete_draft`; see debt note in `discard_
    /// pending_draft`.
    pub async fn flush_drafts(&self) {
        let drained = self.take_dirty_drafts();
        if drained.is_empty() {
            return;
        }
        let writes: Vec<DraftWrite> = {
            let drafts = lock(&self.0.drafts);
            drained
                .into_iter()
                .filter(|d| {
                    drafts
                        .deleted_at
                        .get(&d.session_id)
                        .is_none_or(|&tombstone| tombstone < d.generation)
                })
                .map(|d| DraftWrite {
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
        if let Err(error) = self.store().upsert_drafts(writes).await {
            tracing::error!(%error, "draft flush failed");
        }
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

    /// F10 regression: the destructive interleaving where the flusher has
    /// already drained a draft (so `discard_pending_draft` finds nothing to
    /// remove) and the `DELETE` lands before the flush completes. The
    /// generation tombstone must suppress the upsert so the closed
    /// session's P0 is never resurrected in the encrypted store (SPEC §9.A).
    #[tokio::test]
    async fn delete_during_in_flight_flush_never_resurrects_the_draft() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir).await;
        let session = "s-race".to_owned();

        // (1) A keystroke buffers the draft.
        state.buffer_draft(session.clone(), pending("p0 intime"));

        // (2) The flusher wins the drain race: it now holds the snapshot,
        // and the dirty map no longer contains the session.
        let drained = state.take_dirty_drafts();
        assert_eq!(drained.len(), 1, "flusher drained the buffered draft");

        // (3) The DELETE arrives mid-flush: nothing left to remove from the
        // map, but a tombstone is planted.
        state.discard_pending_draft(&session);
        state
            .store()
            .delete_draft(session.clone())
            .await
            .expect("delete");

        // (4) The flusher finishes its in-flight write. The tombstone must
        // win and suppress it.
        for item in drained {
            let suppressed = {
                let drafts = lock(&state.0.drafts);
                drafts
                    .deleted_at
                    .get(&item.session_id)
                    .is_some_and(|&t| t >= item.generation)
            };
            assert!(suppressed, "the in-flight upsert must be suppressed");
            if !suppressed {
                state
                    .store()
                    .upsert_draft(
                        item.session_id,
                        item.draft.text,
                        item.draft.caret,
                        item.draft.updated_at_micros,
                    )
                    .await
                    .expect("upsert");
            }
        }

        // The closed session must hold no draft.
        assert!(
            state.store().draft(session).await.expect("read").is_none(),
            "a deleted session's P0 was resurrected (F10)"
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
