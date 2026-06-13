// SPDX-License-Identifier: Apache-2.0

//! Shared hub state: store handle, event bus, pairing window, draft
//! autosave buffer, supervised workers.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use fluence_protocol::api::pair::Scope;
use fluence_store::Store;
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

struct Inner {
    config: HubConfig,
    store: Store,
    bus: EventBus,
    started_at: DateTime<Utc>,
    pairing: Mutex<Option<PairingWindow>>,
    dirty_drafts: Mutex<HashMap<String, PendingDraft>>,
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
            dirty_drafts: Mutex::new(HashMap::new()),
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
    /// the latest state matters).
    pub fn buffer_draft(&self, session_id: String, draft: PendingDraft) {
        lock(&self.0.dirty_drafts).insert(session_id, draft);
    }

    /// Takes the pending draft of one session, if any (freshest read).
    #[must_use]
    pub fn pending_draft(&self, session_id: &str) -> Option<PendingDraft> {
        lock(&self.0.dirty_drafts)
            .get(session_id)
            .map(|d| PendingDraft {
                text: d.text.clone(),
                caret: d.caret,
                updated_at_micros: d.updated_at_micros,
            })
    }

    /// Drains every dirty draft for persistence.
    #[must_use]
    pub fn take_dirty_drafts(&self) -> Vec<(String, PendingDraft)> {
        lock(&self.0.dirty_drafts).drain().collect()
    }

    /// Discards the unflushed draft of a closing session.
    pub fn discard_pending_draft(&self, session_id: &str) {
        lock(&self.0.dirty_drafts).remove(session_id);
    }

    /// Flushes all dirty drafts to the store. Called by the periodic
    /// flusher and by graceful shutdown.
    pub async fn flush_drafts(&self) {
        for (session_id, draft) in self.take_dirty_drafts() {
            if let Err(error) = self
                .store()
                .upsert_draft(session_id, draft.text, draft.caret, draft.updated_at_micros)
                .await
            {
                tracing::error!(%error, "draft flush failed");
            }
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
