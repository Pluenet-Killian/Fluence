// SPDX-License-Identifier: Apache-2.0

//! Encrypted persistence for Fluence (SPEC §2.B, §9.A, D-9.1; ADR-0005).
//!
//! Everything that must survive a restart lives here, encrypted at rest
//! with `SQLCipher` (AES-256): paired devices (token **hashes** only),
//! drafts (write-ahead autosave, ≤ 1 s guaranteed loss bound — D-2.6),
//! profiles, and the access journal. **P0 rule**: journal entries carry
//! metadata only, never content (SPEC §9.A).
//!
//! # Architecture (ADR-0005 §5)
//!
//! One dedicated thread owns the `rusqlite` connection; callers talk to it
//! through a command channel ([`Store`] is the async handle). Writes are
//! therefore strictly ordered — which is what makes the draft autosave
//! reasoning sound — and the SQL stays synchronous and readable.
//! Durability: `journal_mode=WAL` + `synchronous=FULL` — the SPEC's
//! «&nbsp;≤ 1 s of typing lost&nbsp;» covers power loss, not just process
//! death, and the autosave rate (≤ 2 writes/s) makes fsync cost invisible.

mod actor;
mod key;
mod schema;
mod types;

use std::path::PathBuf;

use secrecy::SecretString;
use tokio::sync::{mpsc, oneshot};

pub use key::KeySource;
pub use types::{AccessEntry, DeviceRecord, DraftRecord, DraftWrite, NewAccessEntry, NewDevice};

use actor::Command;

/// Store failure modes.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// Underlying database error.
    #[error("store database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// The encryption key could not be obtained or applied.
    #[error("store key error: {0}")]
    Key(String),
    /// The database did not decrypt with the provided key (wrong key or
    /// corrupted file).
    #[error("store does not decrypt with the provided key")]
    WrongKey,
    /// The store thread is gone (after [`Store::close`] or a panic).
    #[error("store is closed")]
    Closed,
    /// A stored value could not be (de)serialized.
    #[error("store serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

/// Where and how to open the store.
#[derive(Debug, Clone)]
pub struct StoreConfig {
    /// Database file path (created if absent, parent directories included).
    pub path: PathBuf,
    /// Where the encryption key comes from.
    pub key: KeySource,
}

/// Async handle to the encrypted store (cheap to clone).
#[derive(Debug, Clone)]
pub struct Store {
    tx: mpsc::Sender<Command>,
}

impl Store {
    /// Opens (or creates) the database, applies the key, runs migrations,
    /// and spawns the owning thread.
    ///
    /// # Errors
    ///
    /// [`StoreError::Key`] when the key source fails;
    /// [`StoreError::WrongKey`] when the file does not decrypt;
    /// [`StoreError::Sqlite`] on any other database failure.
    ///
    /// # Panics
    ///
    /// Panics if the OS refuses to spawn the store thread — an
    /// unrecoverable resource-exhaustion condition at process start.
    pub async fn open(config: StoreConfig) -> Result<Self, StoreError> {
        let (ready_tx, ready_rx) = oneshot::channel();
        let (tx, rx) = mpsc::channel(64);
        std::thread::Builder::new()
            .name("fluence-store".into())
            .spawn(move || actor::run(&config, rx, ready_tx))
            .expect("spawning the store thread never fails");
        ready_rx.await.map_err(|_| StoreError::Closed)??;
        Ok(Self { tx })
    }

    async fn call<R>(
        &self,
        build: impl FnOnce(oneshot::Sender<Result<R, StoreError>>) -> Command,
    ) -> Result<R, StoreError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(build(reply_tx))
            .await
            .map_err(|_| StoreError::Closed)?;
        reply_rx.await.map_err(|_| StoreError::Closed)?
    }

    /// Registers a paired device. Only the token **hash** is stored.
    ///
    /// # Errors
    ///
    /// [`StoreError`] on database failure or closed store.
    pub async fn insert_device(&self, device: NewDevice) -> Result<DeviceRecord, StoreError> {
        self.call(|reply| Command::InsertDevice { device, reply })
            .await
    }

    /// Looks up a **non-revoked** device by token hash (auth path).
    ///
    /// # Errors
    ///
    /// [`StoreError`] on database failure or closed store.
    pub async fn device_by_token_hash(
        &self,
        token_hash: [u8; 32],
    ) -> Result<Option<DeviceRecord>, StoreError> {
        self.call(|reply| Command::DeviceByTokenHash { token_hash, reply })
            .await
    }

    /// Lists every device, revoked included (caregiver space).
    ///
    /// # Errors
    ///
    /// [`StoreError`] on database failure or closed store.
    pub async fn list_devices(&self) -> Result<Vec<DeviceRecord>, StoreError> {
        self.call(|reply| Command::ListDevices { reply }).await
    }

    /// Revokes a device token (effective on the next auth lookup).
    ///
    /// # Errors
    ///
    /// [`StoreError`] on database failure or closed store.
    pub async fn revoke_device(&self, device_id: String) -> Result<(), StoreError> {
        self.call(|reply| Command::RevokeDevice { device_id, reply })
            .await
    }

    /// Inserts or replaces the draft of a session. **P0 content.**
    ///
    /// # Errors
    ///
    /// [`StoreError`] on database failure or closed store.
    pub async fn upsert_draft(
        &self,
        session_id: String,
        text: SecretString,
        caret: u32,
        updated_at_micros: u64,
    ) -> Result<(), StoreError> {
        self.call(|reply| Command::UpsertDraft {
            session_id,
            text,
            caret,
            updated_at_micros,
            reply,
        })
        .await
    }

    /// Inserts or replaces many drafts atomically in a single transaction.
    /// **P0 content.** One fsync covers the whole batch, which is what keeps
    /// the autosave flush bounded under many dirty sessions (D-2.6); an
    /// empty batch is a cheap no-op.
    ///
    /// # Errors
    ///
    /// [`StoreError`] on database failure or closed store.
    pub async fn upsert_drafts(&self, drafts: Vec<DraftWrite>) -> Result<(), StoreError> {
        self.call(|reply| Command::UpsertDrafts { drafts, reply })
            .await
    }

    /// Reads back the draft of a session (session resumption, kill-tests).
    ///
    /// # Errors
    ///
    /// [`StoreError`] on database failure or closed store.
    pub async fn draft(&self, session_id: String) -> Result<Option<DraftRecord>, StoreError> {
        self.call(|reply| Command::Draft { session_id, reply })
            .await
    }

    /// Deletes the draft of a closed session.
    ///
    /// # Errors
    ///
    /// [`StoreError`] on database failure or closed store.
    pub async fn delete_draft(&self, session_id: String) -> Result<(), StoreError> {
        self.call(|reply| Command::DeleteDraft { session_id, reply })
            .await
    }

    /// Purges drafts untouched since `older_than_micros` (F09 disk bound).
    /// Returns the number removed — metadata only, never P0.
    ///
    /// # Errors
    ///
    /// [`StoreError`] on database failure or closed store.
    pub async fn purge_stale_drafts(&self, older_than_micros: u64) -> Result<u64, StoreError> {
        self.call(|reply| Command::PurgeStaleDrafts {
            older_than_micros,
            reply,
        })
        .await
    }

    /// Appends an access-journal entry (metadata only — never P0).
    ///
    /// # Errors
    ///
    /// [`StoreError`] on database failure or closed store.
    pub async fn journal_append(&self, entry: NewAccessEntry) -> Result<(), StoreError> {
        self.call(|reply| Command::JournalAppend { entry, reply })
            .await
    }

    /// Most recent journal entries, newest first.
    ///
    /// # Errors
    ///
    /// [`StoreError`] on database failure or closed store.
    pub async fn journal_recent(&self, limit: u32) -> Result<Vec<AccessEntry>, StoreError> {
        self.call(|reply| Command::JournalRecent { limit, reply })
            .await
    }

    /// Stores a profile (JSON of the experimental contract type).
    ///
    /// # Errors
    ///
    /// [`StoreError`] on database or serialization failure.
    pub async fn put_profile(
        &self,
        profile: fluence_protocol::api::profiles::Profile,
    ) -> Result<(), StoreError> {
        self.call(|reply| Command::PutProfile { profile, reply })
            .await
    }

    /// Reads a profile by id.
    ///
    /// # Errors
    ///
    /// [`StoreError`] on database or deserialization failure.
    pub async fn profile(
        &self,
        profile_id: String,
    ) -> Result<Option<fluence_protocol::api::profiles::Profile>, StoreError> {
        self.call(|reply| Command::GetProfile { profile_id, reply })
            .await
    }

    /// Flushes and stops the store thread. Pending commands sent before
    /// `close` are executed first (channel order).
    ///
    /// # Errors
    ///
    /// [`StoreError::Closed`] if the thread is already gone.
    pub async fn close(self) -> Result<(), StoreError> {
        self.call(|reply| Command::Close { reply }).await
    }
}

#[cfg(test)]
mod tests {
    /// D-10.1: the store is a reusable brick, licensed Apache-2.0.
    #[test]
    fn crate_license_follows_d_10_1() {
        assert_eq!(env!("CARGO_PKG_LICENSE"), "Apache-2.0");
    }
}
