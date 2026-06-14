// SPDX-License-Identifier: Apache-2.0

//! Encrypted backup and restore (PLAN 7.3).
//!
//! A backup is a fully independent `SQLCipher` database, re-encrypted under a
//! [`RecoverySecret`] with `sqlcipher_export` — the live store keeps its
//! machine key, the archive travels with the printable kit. Restore is the
//! exact same step in reverse, which is why it is guarded by a round-trip
//! **test** (`tests/backup_roundtrips.rs`), not a promise (PLAN 7.3: « la
//! restauration est un test CI »). Both operate on database files directly, so
//! the hub must be stopped first — a clean, lock-free copy, and a restore that
//! is safe to retry because it never writes in place.

use std::path::Path;

use rusqlite::Connection;
use secrecy::ExposeSecret;

use crate::recovery::RecoverySecret;
use crate::{KeySource, StoreError, key};

/// Backs the live store up to `archive_path`, re-encrypted under `secret`.
///
/// `archive_path` must not exist — a backup never overwrites. The live store
/// stays keyed by `store_key`; the archive is keyed by the recovery secret, so
/// it restores on any machine that has the kit.
///
/// # Errors
///
/// [`StoreError::Key`] when the target already exists or the store key is
/// unavailable; [`StoreError::WrongKey`] when the store does not decrypt;
/// [`StoreError::Sqlite`] on any database failure.
pub fn back_up(
    store_path: &Path,
    store_key: &KeySource,
    archive_path: &Path,
    secret: &RecoverySecret,
) -> Result<(), StoreError> {
    if archive_path.exists() {
        return Err(StoreError::Key(format!(
            "backup target already exists: {}",
            archive_path.display()
        )));
    }
    let store_key_hex = key::load_or_create(store_key)?;
    let result = reencrypt(
        store_path,
        store_key_hex.expose_secret(),
        archive_path,
        secret.key_hex().expose_secret(),
    );
    if result.is_err() {
        // A partial archive is worse than none — it looks restorable but is
        // not. Best effort: leave the filesystem as we found it.
        let _ = std::fs::remove_file(archive_path);
    }
    result
}

/// Restores `archive_path` (keyed by `secret`) into a fresh store at
/// `store_path`, re-encrypted under this machine's `store_key`.
///
/// `store_path` must not exist: the operator moves any current store aside
/// first, so a failed restore can never destroy live data in place.
///
/// # Errors
///
/// [`StoreError::Key`] when the target exists, the archive is missing, or the
/// machine key is unavailable; [`StoreError::WrongKey`] when `secret` does not
/// decrypt the archive; [`StoreError::Sqlite`] on any database failure.
pub fn restore(
    archive_path: &Path,
    secret: &RecoverySecret,
    store_path: &Path,
    store_key: &KeySource,
) -> Result<(), StoreError> {
    if store_path.exists() {
        return Err(StoreError::Key(format!(
            "restore target already exists (move it aside first): {}",
            store_path.display()
        )));
    }
    if !archive_path.exists() {
        return Err(StoreError::Key(format!(
            "backup archive not found: {}",
            archive_path.display()
        )));
    }
    let store_key_hex = key::load_or_create(store_key)?;
    let result = reencrypt(
        archive_path,
        secret.key_hex().expose_secret(),
        store_path,
        store_key_hex.expose_secret(),
    );
    if result.is_err() {
        let _ = std::fs::remove_file(store_path);
    }
    result
}

/// Opens `src` (raw key `src_key_hex`) and writes a fully independent,
/// re-encrypted copy at `dst` (raw key `dst_key_hex`) via `sqlcipher_export`.
/// `dst` must be absent. `sqlcipher_export` copies schema and rows but **not**
/// `user_version`, so it is carried across explicitly — otherwise the restored
/// store would re-run migrations against an already-migrated schema.
fn reencrypt(
    src: &Path,
    src_key_hex: &str,
    dst: &Path,
    dst_key_hex: &str,
) -> Result<(), StoreError> {
    let conn = Connection::open(src)?;
    // Raw key — must be the first statement, exactly as `Store::open` does.
    conn.pragma_update(None, "key", format!("x'{src_key_hex}'"))?;
    // Wrong key (or a non-database file) surfaces on the first real read.
    if conn
        .query_row("SELECT count(*) FROM sqlite_master", [], |row| {
            row.get::<_, i64>(0)
        })
        .is_err()
    {
        return Err(StoreError::WrongKey);
    }

    let dst_str = dst
        .to_str()
        .ok_or_else(|| StoreError::Key("backup path is not valid UTF-8".into()))?;
    // The key is validated hex (safe to inline); the path is bound, since it
    // may contain spaces or, on Windows, backslashes.
    conn.execute(
        &format!("ATTACH DATABASE ?1 AS fluence_backup KEY \"x'{dst_key_hex}'\""),
        rusqlite::params![dst_str],
    )?;

    let exported = export_main_into_attached(&conn);
    // Always detach so the file handle is freed (Windows cannot delete an open
    // file), but surface the export error first — it is the meaningful one.
    let detached = conn.execute("DETACH DATABASE fluence_backup", []);
    exported?;
    detached?;
    Ok(())
}

/// Runs `sqlcipher_export` into the attached `fluence_backup` schema and
/// carries `user_version` across.
fn export_main_into_attached(conn: &Connection) -> Result<(), StoreError> {
    conn.query_row("SELECT sqlcipher_export('fluence_backup')", [], |_| Ok(()))?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    // PRAGMA cannot bind a parameter; the value is an integer read from the
    // source database, not from input.
    conn.execute_batch(&format!("PRAGMA fluence_backup.user_version = {version}"))?;
    Ok(())
}
