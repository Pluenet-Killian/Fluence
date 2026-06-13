// SPDX-License-Identifier: Apache-2.0

//! Shared hub state: store handle, event bus, pairing window, draft
//! autosave buffer, supervised workers.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use fluence_inference::{CancelToken, LlmBackend, UnavailableBackend};
use fluence_ngram::NgramModel;
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

/// Hard cap on a draft's text, in bytes (F09). A real composed message is
/// a few hundred bytes; 64 KiB is far above any legitimate draft while
/// keeping a flood of giant P0 `SecretString`s in RAM impossible. Enforced
/// before any P0 allocation (`put_draft`); also the HTTP body limit (G7).
pub const MAX_DRAFT_TEXT_BYTES: usize = 64 * 1024;

/// Most distinct sessions whose unflushed draft the hub holds in RAM at
/// once (F09). A household composes a handful at a time; past this, a
/// Control device looping `PUT` under fresh ids is curbed. Overflow never
/// drops or blocks a write — it forces an immediate flush so the P0 leaves
/// RAM for the encrypted store (loss bound stays ≤ 1 s, D-2.6).
pub const MAX_DIRTY_DRAFTS: usize = 256;

/// Drafts untouched for this long are purged from disk (F09 disk bound).
/// Generous: only abandoned/orphaned drafts (e.g. fabricated session ids)
/// age out; a live conversation re-touches its draft far more often.
pub const DRAFT_DISK_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// How often the on-disk stale-draft purge runs (cheap indexed delete, off
/// the hot path).
pub const DRAFT_PURGE_PERIOD: Duration = Duration::from_secs(60 * 60);

/// Concurrent `/ws` connections a single device may hold (F15). One real
/// client opens one channel; a handful covers reconnect races and multiple
/// windows. Past this, a paired-but-misbehaving device is curbed before it
/// can exhaust file descriptors and take the keyboard path down (SPEC §2.C).
pub const WS_MAX_PER_DEVICE: u32 = 8;

/// Hub-wide ceiling on concurrent `/ws` connections (F15). A household
/// backstop so no single device — even within its own quota — exhausts the
/// process; well under a default 1024-FD limit.
pub const WS_MAX_TOTAL: u32 = 128;

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

/// Open-`/ws` accounting (F15): per-device counts and their running total,
/// guarded by one mutex so an admission decision is atomic.
#[derive(Default)]
struct WsCounters {
    per_device: HashMap<String, u32>,
    total: u32,
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
    /// Concurrent `/ws` connections, per device and in total (F15 `DoS`
    /// guard).
    ws_counters: Mutex<WsCounters>,
    /// The LLM backend the acceleration engine calls. Defaults to
    /// [`UnavailableBackend`] until a `worker-llm` is configured, so the engine
    /// degrades to `fallback` rather than failing (D-2.6).
    engine: Arc<dyn LlmBackend>,
    /// The always-loaded n-gram fallback (D-2.6 « le clavier parle toujours »):
    /// serves predictions whenever the LLM backend is unavailable.
    fallback: Arc<NgramModel>,
    /// Per-slot suggestion cancellation (§5.A: the debounce lives in the
    /// server). Each `(session, slot)` maps to its in-flight generation's
    /// token, tagged with a generation number so a finishing task clears only
    /// its own entry, never a newer request's.
    suggest_slots: Mutex<HashMap<(String, String), (u64, CancelToken)>>,
    /// Monotonic source for suggestion-slot generations.
    suggest_generation: AtomicU64,
}

/// Cheaply cloneable hub state.
#[derive(Clone)]
pub struct AppState(Arc<Inner>);

impl AppState {
    /// Assembles the state with the default backend ([`UnavailableBackend`])
    /// and an empty n-gram fallback. Workers register afterwards
    /// ([`AppState::register_worker`]).
    #[must_use]
    pub fn new(config: HubConfig, store: Store, bus: EventBus) -> Self {
        Self::new_with(
            config,
            store,
            bus,
            Arc::new(UnavailableBackend),
            Arc::new(NgramModel::new()),
        )
    }

    /// Assembles the state with an explicit LLM backend and n-gram fallback.
    /// Tests inject a stub backend or a trained fallback; production wires the
    /// real worker bridge here once it exists.
    #[must_use]
    pub fn new_with(
        config: HubConfig,
        store: Store,
        bus: EventBus,
        engine: Arc<dyn LlmBackend>,
        fallback: Arc<NgramModel>,
    ) -> Self {
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
            ws_counters: Mutex::new(WsCounters::default()),
            engine,
            fallback,
            suggest_slots: Mutex::new(HashMap::new()),
            suggest_generation: AtomicU64::new(0),
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

    /// The LLM backend the acceleration engine calls.
    #[must_use]
    pub fn engine(&self) -> &Arc<dyn LlmBackend> {
        &self.0.engine
    }

    /// The n-gram fallback predictor (D-2.6 « le clavier parle toujours »).
    #[must_use]
    pub fn fallback(&self) -> &Arc<NgramModel> {
        &self.0.fallback
    }

    /// Registers a new suggestion on `(session, slot)`, cancelling any in-flight
    /// one on the same slot (the per-slot debounce lives in the server, §5.A).
    /// Returns the request's generation number and its cancellation token.
    #[must_use]
    pub fn supersede_slot(&self, session: &str, slot: &str) -> (u64, CancelToken) {
        let generation = self.0.suggest_generation.fetch_add(1, Ordering::Relaxed);
        let token = CancelToken::new();
        let previous = lock(&self.0.suggest_slots).insert(
            (session.to_owned(), slot.to_owned()),
            (generation, token.clone()),
        );
        if let Some((_, previous_token)) = previous {
            previous_token.cancel();
        }
        (generation, token)
    }

    /// Clears a slot once its generation ends — but only if it is still the
    /// current one, so a newer in-flight request is never forgotten.
    pub fn clear_slot(&self, session: &str, slot: &str, generation: u64) {
        let mut slots = lock(&self.0.suggest_slots);
        let key = (session.to_owned(), slot.to_owned());
        if slots.get(&key).is_some_and(|(g, _)| *g == generation) {
            slots.remove(&key);
        }
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
    ///
    /// Returns `true` when this write pushed the buffer past
    /// [`MAX_DIRTY_DRAFTS`] (F09): the caller must then flush immediately to
    /// bring RAM back down. Updating an already-buffered session never
    /// overflows, so an active session's keystrokes are never throttled —
    /// only a brand-new session beyond the cap triggers it.
    #[must_use = "an overflow means the caller must flush now to bound RAM (F09)"]
    pub fn buffer_draft(&self, session_id: String, draft: PendingDraft) -> bool {
        let generation = self.0.draft_generation.fetch_add(1, Ordering::Relaxed);
        let mut drafts = lock(&self.0.drafts);
        drafts.deleted_at.remove(&session_id);
        let is_new_session = !drafts.dirty.contains_key(&session_id);
        drafts
            .dirty
            .insert(session_id, BufferedDraft { draft, generation });
        is_new_session && drafts.dirty.len() > MAX_DIRTY_DRAFTS
    }

    /// Reserves a `/ws` slot for `device_id` if both the per-device and
    /// hub-wide ceilings allow it, returning a guard that releases the slot
    /// on drop (F15). `None` means the device is at quota (or the hub is
    /// saturated): the connection must be refused *before* upgrade, so no
    /// task, no `broadcast::Receiver` and no file descriptor is committed.
    /// Admission is decided under one lock — two simultaneous upgrades
    /// cannot both take the last slot.
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
        counters
            .per_device
            .insert(device_id.to_owned(), device_count + 1);
        Some(WsConnectionGuard {
            state: self.clone(),
            device_id: device_id.to_owned(),
        })
    }

    /// Releases one `/ws` slot (called only by [`WsConnectionGuard::drop`]).
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

    /// Spawns the periodic on-disk stale-draft purge (F09 disk bound), which
    /// reclaims drafts no client has touched within [`DRAFT_DISK_TTL`] —
    /// independent of the AI/worker health, so the keyboard guarantee is
    /// untouched (SPEC §2.C). A store error is logged and retried next tick.
    pub fn spawn_draft_purger(&self) {
        let state = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(DRAFT_PURGE_PERIOD);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            let ttl_micros = u64::try_from(DRAFT_DISK_TTL.as_micros()).unwrap_or(u64::MAX);
            loop {
                tick.tick().await;
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
}

/// Releases its reserved `/ws` slot on drop (F15). Tying the slot's
/// lifetime to a stack value guarantees the count is decremented on
/// *every* exit of the connection task — clean close, error, or panic — so
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

        let _ = state.buffer_draft(session.clone(), pending("p0 intime"));
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
        let _ = state.buffer_draft("s1".to_owned(), pending("pas encore ecrite"));

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

        let _ = state.buffer_draft(session.clone(), pending("first"));
        state.discard_pending_draft(&session);
        // A fresh keystroke after the close — strictly newer generation.
        let _ = state.buffer_draft(session.clone(), pending("second"));

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

        let _ = state.buffer_draft(session.clone(), pending("bonjour"));
        state.flush_drafts().await;

        let restored = state
            .store()
            .draft(session)
            .await
            .expect("read")
            .expect("present");
        assert_eq!(restored.text.expose_secret(), "bonjour");
    }

    /// F09: the dirty-buffer cardinality cap. Buffering up to
    /// `MAX_DIRTY_DRAFTS` distinct sessions never signals overflow; the
    /// *next* brand-new session does, so the caller flushes and RAM stays
    /// bounded. Re-buffering an already-dirty session never overflows — an
    /// active conversation's keystrokes are never throttled (D-2.6).
    #[tokio::test]
    async fn a_new_session_past_the_cap_signals_overflow() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir).await;

        for i in 0..MAX_DIRTY_DRAFTS {
            assert!(
                !state.buffer_draft(format!("s{i}"), pending("x")),
                "session {i} is within the cap and must not overflow"
            );
        }
        assert!(
            state.buffer_draft("s-overflow".to_owned(), pending("x")),
            "a new session beyond MAX_DIRTY_DRAFTS must signal overflow (F09)"
        );
        assert!(
            !state.buffer_draft("s0".to_owned(), pending("y")),
            "re-buffering an already-dirty session must never overflow"
        );
    }

    /// F15: the per-device `/ws` ceiling. A device may hold up to
    /// `WS_MAX_PER_DEVICE` slots; the next acquire is refused. A distinct
    /// device keeps its own quota, and dropping a guard frees exactly one
    /// slot so a disconnect lets the device reconnect (RAII release).
    #[tokio::test]
    async fn ws_admission_enforces_the_per_device_ceiling() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir).await;

        let mut guards = Vec::new();
        for _ in 0..WS_MAX_PER_DEVICE {
            guards.push(
                state
                    .try_acquire_ws("dev-a")
                    .expect("within the per-device quota"),
            );
        }
        assert!(
            state.try_acquire_ws("dev-a").is_none(),
            "a device past WS_MAX_PER_DEVICE must be refused (F15)"
        );
        assert!(
            state.try_acquire_ws("dev-b").is_some(),
            "a distinct device has its own quota"
        );

        guards.pop(); // a connection ends: its guard drops, releasing one slot
        assert!(
            state.try_acquire_ws("dev-a").is_some(),
            "a freed slot lets the device reconnect (RAII release)"
        );
    }

    /// F15: the hub-wide `/ws` ceiling caps total concurrency even when load
    /// is spread across many devices, each within its own per-device quota.
    #[tokio::test]
    async fn ws_admission_enforces_the_hub_wide_ceiling() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir).await;

        let per_device = usize::try_from(WS_MAX_PER_DEVICE).expect("cap fits usize");
        let total = usize::try_from(WS_MAX_TOTAL).expect("cap fits usize");
        let devices = total.div_ceil(per_device);

        let mut guards = Vec::new();
        for d in 0..devices {
            for _ in 0..per_device {
                if let Some(guard) = state.try_acquire_ws(&format!("dev-{d}")) {
                    guards.push(guard);
                }
            }
        }
        assert_eq!(
            guards.len(),
            total,
            "exactly the hub-wide ceiling is admitted, no more"
        );
        assert!(
            state.try_acquire_ws("dev-fresh").is_none(),
            "the hub-wide ceiling caps total concurrency (F15)"
        );
    }

    /// §5.A per-slot debounce: a new suggestion on the same slot cancels the
    /// in-flight one. The previous token must trip.
    #[tokio::test]
    async fn superseding_a_slot_cancels_the_previous_request() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir).await;

        let (_, first) = state.supersede_slot("s1", "main");
        assert!(!first.is_cancelled());
        let (_, _second) = state.supersede_slot("s1", "main");
        assert!(
            first.is_cancelled(),
            "the previous same-slot request must be cancelled"
        );
    }

    /// A different slot is independent: superseding `alt` never cancels `main`.
    #[tokio::test]
    async fn a_different_slot_is_not_cancelled() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir).await;

        let (_, main_token) = state.supersede_slot("s1", "main");
        let (_, _alt) = state.supersede_slot("s1", "alt");
        assert!(!main_token.is_cancelled(), "a distinct slot is untouched");
    }

    /// A finishing task's `clear_slot` must not evict a newer request's token:
    /// a stale clear is a no-op, so the current request stays cancellable.
    #[tokio::test]
    async fn a_stale_clear_does_not_drop_the_current_request() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir).await;

        let (stale_generation, _first) = state.supersede_slot("s1", "main");
        let (_, current) = state.supersede_slot("s1", "main"); // cancels _first
        state.clear_slot("s1", "main", stale_generation); // must be a no-op

        // The current token is still tracked: a third request cancels it.
        let (_, _third) = state.supersede_slot("s1", "main");
        assert!(
            current.is_cancelled(),
            "a stale clear must not forget the current request"
        );
    }
}
