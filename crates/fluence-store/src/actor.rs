// SPDX-License-Identifier: Apache-2.0

//! The store actor: one thread owns the connection, commands arrive in
//! order, every command answers through its own `oneshot`.

use chrono::{DateTime, Utc};
use fluence_protocol::api::profiles::Profile;
use rusqlite::{Connection, OptionalExtension, params};
use secrecy::{ExposeSecret, SecretString};
use tokio::sync::{mpsc, oneshot};

use crate::types::{AccessEntry, DeviceRecord, DraftRecord, DraftWrite, NewAccessEntry, NewDevice};
use crate::{StoreConfig, StoreError, key, schema};

type Reply<R> = oneshot::Sender<Result<R, StoreError>>;

/// Hard cap on retained access-journal rows (ADR-0005 §5; F26).
///
/// The journal is *append-only metadata* written on uncontrolled paths —
/// notably `auth.rejected`, which an unauthenticated loopback client can
/// hammer (`auth.rs`, `ws.rs`). Without a bound the table grows forever:
/// it fills the home disk *and* — worse, because it bites first — floods
/// the single store connection's fsync queue, starving the draft flusher
/// and threatening the «&nbsp;≤ 1 s lost&nbsp;» guarantee (D-2.6). Trimming
/// on every append makes growth structurally impossible and keeps the WAL
/// and its checkpoints small, so the keyboard path stays fast. 5 000 rows
/// is far more history than the caregiver UI ever shows yet trivial on
/// disk (< 1 MB encrypted).
const JOURNAL_MAX_ROWS: i64 = 5_000;

/// Commands the [`crate::Store`] handle sends to the actor.
pub enum Command {
    /// Register a paired device.
    InsertDevice {
        /// Device to insert.
        device: NewDevice,
        /// Result channel.
        reply: Reply<DeviceRecord>,
    },
    /// Auth lookup by token hash (non-revoked only).
    DeviceByTokenHash {
        /// SHA-256 of the presented token.
        token_hash: [u8; 32],
        /// Result channel.
        reply: Reply<Option<DeviceRecord>>,
    },
    /// Full device list (revoked included).
    ListDevices {
        /// Result channel.
        reply: Reply<Vec<DeviceRecord>>,
    },
    /// Revoke one device.
    RevokeDevice {
        /// Device to revoke.
        device_id: String,
        /// Result channel.
        reply: Reply<()>,
    },
    /// Insert/replace a session draft.
    UpsertDraft {
        /// Session the draft belongs to.
        session_id: String,
        /// Draft text (P0).
        text: SecretString,
        /// Caret position.
        caret: u32,
        /// Last-keystroke timestamp (µs).
        updated_at_micros: u64,
        /// Result channel.
        reply: Reply<()>,
    },
    /// Insert/replace many drafts in a single transaction (one fsync for
    /// the whole batch — the autosave flush path, D-2.6).
    UpsertDrafts {
        /// Drafts to persist, in the order they should be written.
        drafts: Vec<DraftWrite>,
        /// Result channel.
        reply: Reply<()>,
    },
    /// Read a session draft back.
    Draft {
        /// Session to read.
        session_id: String,
        /// Result channel.
        reply: Reply<Option<DraftRecord>>,
    },
    /// Delete a session draft (session closed).
    DeleteDraft {
        /// Session whose draft dies with it.
        session_id: String,
        /// Result channel.
        reply: Reply<()>,
    },
    /// Delete drafts untouched since `older_than_micros` (TTL purge, F09).
    PurgeStaleDrafts {
        /// Cutoff: drafts with `updated_at_micros < older_than_micros` die.
        older_than_micros: u64,
        /// Result channel: number of drafts purged (metadata, never P0).
        reply: Reply<u64>,
    },
    /// Erase all personal content (drafts + profiles) and reclaim the pages
    /// (SPEC §9.A «&nbsp;oubli&nbsp;»).
    PurgeContent {
        /// Result channel: rows removed (metadata, never P0).
        reply: Reply<u64>,
    },
    /// Append a journal entry.
    JournalAppend {
        /// Entry to append.
        entry: NewAccessEntry,
        /// Result channel.
        reply: Reply<()>,
    },
    /// Read recent journal entries (newest first).
    JournalRecent {
        /// Maximum entries.
        limit: u32,
        /// Result channel.
        reply: Reply<Vec<AccessEntry>>,
    },
    /// Store a profile.
    PutProfile {
        /// Profile (contract type, serialized as JSON).
        profile: Profile,
        /// Result channel.
        reply: Reply<()>,
    },
    /// Read a profile.
    GetProfile {
        /// Profile id.
        profile_id: String,
        /// Result channel.
        reply: Reply<Option<Profile>>,
    },
    /// Flush and stop the thread.
    Close {
        /// Result channel.
        reply: Reply<()>,
    },
}

/// Thread entry point: open, key, migrate, then serve commands until
/// `Close` or every handle is dropped.
pub fn run(
    config: &StoreConfig,
    mut rx: mpsc::Receiver<Command>,
    ready: oneshot::Sender<Result<(), StoreError>>,
) {
    let mut conn = match open(config) {
        Ok(conn) => {
            // The receiver only drops if `Store::open` was cancelled.
            let _ = ready.send(Ok(()));
            conn
        }
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };

    while let Some(command) = rx.blocking_recv() {
        if dispatch(&mut conn, command) {
            break;
        }
    }
    // WAL checkpoint on the way out keeps the main file self-contained.
    let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)");
}

/// Opens the database with the key applied *before* any other statement,
/// verifies decryption, sets durability pragmas, migrates.
fn open(config: &StoreConfig) -> Result<Connection, StoreError> {
    if let Some(parent) = config.path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| StoreError::Key(format!("cannot create store dir: {e}")))?;
    }
    let key_hex = key::load_or_create(&config.key)?;
    let mut conn = Connection::open(&config.path)?;

    // SQLCipher raw key — must be the first statement on the connection.
    conn.pragma_update(None, "key", format!("x'{}'", key_hex.expose_secret()))?;

    // Wrong key (or non-database file) surfaces on the first real read.
    let probe: Result<i64, _> =
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |row| row.get(0));
    if probe.is_err() {
        return Err(StoreError::WrongKey);
    }

    // Durability (ADR-0005 §5): WAL for write-ahead semantics, FULL so a
    // committed autosave survives power loss, not just process death.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "FULL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;

    schema::migrate(&mut conn)?;
    Ok(conn)
}

/// Executes one command. Returns `true` when the actor must stop.
fn dispatch(conn: &mut Connection, command: Command) -> bool {
    match command {
        Command::InsertDevice { device, reply } => {
            let _ = reply.send(insert_device(conn, &device));
        }
        Command::DeviceByTokenHash { token_hash, reply } => {
            let _ = reply.send(device_by_token_hash(conn, token_hash));
        }
        Command::ListDevices { reply } => {
            let _ = reply.send(list_devices(conn));
        }
        Command::RevokeDevice { device_id, reply } => {
            let _ = reply.send(revoke_device(conn, &device_id));
        }
        Command::UpsertDraft {
            session_id,
            text,
            caret,
            updated_at_micros,
            reply,
        } => {
            let _ = reply.send(upsert_draft(
                conn,
                &session_id,
                &text,
                caret,
                updated_at_micros,
            ));
        }
        Command::UpsertDrafts { drafts, reply } => {
            let _ = reply.send(upsert_drafts(conn, &drafts));
        }
        Command::Draft { session_id, reply } => {
            let _ = reply.send(draft(conn, &session_id));
        }
        Command::DeleteDraft { session_id, reply } => {
            let _ = reply.send(delete_draft(conn, &session_id));
        }
        Command::PurgeStaleDrafts {
            older_than_micros,
            reply,
        } => {
            let _ = reply.send(purge_stale_drafts(conn, older_than_micros));
        }
        Command::PurgeContent { reply } => {
            let _ = reply.send(purge_content(conn));
        }
        Command::JournalAppend { entry, reply } => {
            let _ = reply.send(journal_append(conn, &entry));
        } // takes &mut: append + trim commit in one transaction (one fsync)
        Command::JournalRecent { limit, reply } => {
            let _ = reply.send(journal_recent(conn, limit));
        }
        Command::PutProfile { profile, reply } => {
            let _ = reply.send(put_profile(conn, &profile));
        }
        Command::GetProfile { profile_id, reply } => {
            let _ = reply.send(get_profile(conn, &profile_id));
        }
        Command::Close { reply } => {
            let _ = reply.send(Ok(()));
            return true;
        }
    }
    false
}

fn insert_device(conn: &Connection, device: &NewDevice) -> Result<DeviceRecord, StoreError> {
    let created_at = Utc::now();
    conn.execute(
        "INSERT INTO devices (device_id, token_hash, name, kind, scope, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            device.device_id,
            device.token_hash.as_slice(),
            device.name,
            serde_plain(&device.kind)?,
            serde_plain(&device.scope)?,
            created_at.to_rfc3339(),
        ],
    )?;
    Ok(DeviceRecord {
        device_id: device.device_id.clone(),
        name: device.name.clone(),
        kind: device.kind,
        scope: device.scope,
        created_at,
        revoked_at: None,
    })
}

fn device_by_token_hash(
    conn: &Connection,
    token_hash: [u8; 32],
) -> Result<Option<DeviceRecord>, StoreError> {
    conn.query_row(
        "SELECT device_id, name, kind, scope, created_at, revoked_at
         FROM devices WHERE token_hash = ?1 AND revoked_at IS NULL",
        params![token_hash.as_slice()],
        row_to_device,
    )
    .optional()
    .map_err(StoreError::from)
}

fn list_devices(conn: &Connection) -> Result<Vec<DeviceRecord>, StoreError> {
    let mut statement = conn.prepare(
        "SELECT device_id, name, kind, scope, created_at, revoked_at
         FROM devices ORDER BY created_at",
    )?;
    let rows = statement.query_map([], row_to_device)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::from)
}

fn revoke_device(conn: &Connection, device_id: &str) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE devices SET revoked_at = ?1 WHERE device_id = ?2 AND revoked_at IS NULL",
        params![Utc::now().to_rfc3339(), device_id],
    )?;
    Ok(())
}

fn upsert_draft(
    conn: &Connection,
    session_id: &str,
    text: &SecretString,
    caret: u32,
    updated_at_micros: u64,
) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO drafts (session_id, text, caret, updated_at_micros)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(session_id) DO UPDATE
         SET text = excluded.text, caret = excluded.caret,
             updated_at_micros = excluded.updated_at_micros",
        params![session_id, text.expose_secret(), caret, updated_at_micros],
    )?;
    Ok(())
}

/// Persists a whole batch of drafts inside a single transaction, so the
/// `synchronous=FULL` fsync cost is paid **once** for the batch instead of
/// once per draft. This bounds the autosave flush duration regardless of
/// how many sessions are dirty (D-2.6): a flusher tick no longer stretches
/// linearly with the session count, which kept the «&nbsp;≤ 1 s lost&nbsp;»
/// window from blowing up under a burst of distinct sessions. An empty
/// batch is a no-op (no transaction, no fsync). The whole batch commits or
/// rolls back atomically — a partial flush never leaves the store in a
/// state the loss-bound reasoning does not cover.
fn upsert_drafts(conn: &mut Connection, drafts: &[DraftWrite]) -> Result<(), StoreError> {
    if drafts.is_empty() {
        return Ok(());
    }
    let tx = conn.transaction()?;
    {
        let mut statement = tx.prepare_cached(
            "INSERT INTO drafts (session_id, text, caret, updated_at_micros)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(session_id) DO UPDATE
             SET text = excluded.text, caret = excluded.caret,
                 updated_at_micros = excluded.updated_at_micros",
        )?;
        for draft in drafts {
            statement.execute(params![
                draft.session_id,
                draft.text.expose_secret(),
                draft.caret,
                draft.updated_at_micros,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

fn draft(conn: &Connection, session_id: &str) -> Result<Option<DraftRecord>, StoreError> {
    conn.query_row(
        "SELECT text, caret, updated_at_micros FROM drafts WHERE session_id = ?1",
        params![session_id],
        |row| {
            Ok(DraftRecord {
                text: SecretString::from(row.get::<_, String>(0)?),
                caret: row.get(1)?,
                updated_at_micros: row.get(2)?,
            })
        },
    )
    .optional()
    .map_err(StoreError::from)
}

fn delete_draft(conn: &Connection, session_id: &str) -> Result<(), StoreError> {
    conn.execute(
        "DELETE FROM drafts WHERE session_id = ?1",
        params![session_id],
    )?;
    Ok(())
}

/// Deletes every draft whose last keystroke is older than the cutoff
/// (F09 disk bound), via the `idx_drafts_updated_at` index. Returns the
/// count purged — a plain number, safe to log (no P0 ever leaves here).
fn purge_stale_drafts(conn: &Connection, older_than_micros: u64) -> Result<u64, StoreError> {
    let purged = conn.execute(
        "DELETE FROM drafts WHERE updated_at_micros < ?1",
        params![older_than_micros],
    )?;
    Ok(u64::try_from(purged).unwrap_or(u64::MAX))
}

/// Erases every draft and profile — the SPEC §9.A content purge
/// («&nbsp;oubli&nbsp;») — then `VACUUM`s so the freed pages, which still hold
/// encrypted P0, are rewritten out of the file rather than lingering as
/// reusable free pages. Devices and the access journal are kept: the user
/// erased their *content*, not their device pairings or the audit trail.
/// Returns the row count, which is metadata and safe to surface.
fn purge_content(conn: &mut Connection) -> Result<u64, StoreError> {
    let tx = conn.transaction()?;
    let drafts = tx.execute("DELETE FROM drafts", [])?;
    let profiles = tx.execute("DELETE FROM profiles", [])?;
    tx.commit()?;
    // VACUUM rewrites the database (dropping freed pages) and cannot run
    // inside a transaction — hence after the commit above.
    conn.execute_batch("VACUUM")?;
    Ok(u64::try_from(drafts + profiles).unwrap_or(u64::MAX))
}

/// Appends a journal entry and trims the table back under
/// [`JOURNAL_MAX_ROWS`] in the *same* transaction, so the access journal
/// is bounded by construction (F26).
///
/// Insert and trim share one commit — hence one fsync — so a flood of
/// `auth.rejected` writes cannot multiply IO on the connection the draft
/// flusher depends on. The trim deletes by `id` (the rowid: an integer
/// primary key, already indexed), and `id` is `AUTOINCREMENT`, so the
/// high-water mark keeps rising across deletes and reopens. In steady
/// state at most one row is evicted per append, so this is amortized
/// O(1); the first append after a migration from an over-budget table
/// pays a one-off bulk delete.
fn journal_append(conn: &mut Connection, entry: &NewAccessEntry) -> Result<(), StoreError> {
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO access_journal (at, device_id, action, detail) VALUES (?1, ?2, ?3, ?4)",
        params![
            Utc::now().to_rfc3339(),
            entry.device_id,
            entry.action,
            entry.detail
        ],
    )?;
    // Keep only the newest JOURNAL_MAX_ROWS rows. `max(id)` is the
    // monotonic high-water mark; anything more than the budget below it is
    // stale. NULL-safe: on an (impossible here, post-insert) empty table
    // `max(id)` is NULL and the predicate matches nothing.
    tx.execute(
        "DELETE FROM access_journal
         WHERE id <= (SELECT max(id) FROM access_journal) - ?1",
        params![JOURNAL_MAX_ROWS],
    )?;
    tx.commit()?;
    Ok(())
}

fn journal_recent(conn: &Connection, limit: u32) -> Result<Vec<AccessEntry>, StoreError> {
    let mut statement = conn.prepare(
        "SELECT id, at, device_id, action, detail
         FROM access_journal ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = statement.query_map(params![limit], |row| {
        Ok(AccessEntry {
            id: row.get(0)?,
            at: parse_rfc3339(row, 1)?,
            device_id: row.get(2)?,
            action: row.get(3)?,
            detail: row.get(4)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::from)
}

fn put_profile(conn: &Connection, profile: &Profile) -> Result<(), StoreError> {
    let data = serde_json::to_string(profile)?;
    conn.execute(
        "INSERT INTO profiles (profile_id, data) VALUES (?1, ?2)
         ON CONFLICT(profile_id) DO UPDATE SET data = excluded.data",
        params![profile.id.0, data],
    )?;
    Ok(())
}

fn get_profile(conn: &Connection, profile_id: &str) -> Result<Option<Profile>, StoreError> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT data FROM profiles WHERE profile_id = ?1",
            params![profile_id],
            |row| row.get(0),
        )
        .optional()?;
    raw.map(|data| serde_json::from_str(&data).map_err(StoreError::from))
        .transpose()
}

/// Maps a device row (column order of the device queries above).
fn row_to_device(row: &rusqlite::Row<'_>) -> rusqlite::Result<DeviceRecord> {
    Ok(DeviceRecord {
        device_id: row.get(0)?,
        name: row.get(1)?,
        kind: parse_plain(row, 2)?,
        scope: parse_plain(row, 3)?,
        created_at: parse_rfc3339(row, 4)?,
        revoked_at: row
            .get::<_, Option<String>>(5)?
            .map(|raw| parse_rfc3339_str(&raw, 5))
            .transpose()?,
    })
}

/// Serializes an enum to its bare wire string (`"control"`, not
/// `"\"control\""`).
fn serde_plain<T: serde::Serialize>(value: &T) -> Result<String, StoreError> {
    let json = serde_json::to_value(value)?;
    json.as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| StoreError::Key("enum did not serialize to a string".into()))
}

/// Parses an enum from its bare wire string.
fn parse_plain<T: serde::de::DeserializeOwned>(
    row: &rusqlite::Row<'_>,
    index: usize,
) -> rusqlite::Result<T> {
    let stored: String = row.get(index)?;
    serde_json::from_value(serde_json::Value::String(stored)).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn parse_rfc3339(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<DateTime<Utc>> {
    let stored: String = row.get(index)?;
    parse_rfc3339_str(&stored, index)
}

fn parse_rfc3339_str(raw: &str, index: usize) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                index,
                rusqlite::types::Type::Text,
                Box::new(e),
            )
        })
}
