// SPDX-License-Identifier: Apache-2.0

//! Versioned migrations over `SQLite`'s `user_version` pragma.
//!
//! Hand-rolled on purpose (ADR-0005 §5 amendment): refinery pins an
//! incompatible `libsqlite3-sys`, and four tables do not justify a
//! framework. The rules are the same ones a framework enforces: scripts
//! are **append-only** (never edit a shipped migration), each runs in a
//! transaction, `user_version` records progress.

use rusqlite::Connection;

use crate::StoreError;

/// Append-only migration scripts. Index `i` migrates `user_version == i`
/// to `i + 1`.
const MIGRATIONS: &[&str] = &[
    // v0 → v1: initial schema.
    "
    CREATE TABLE devices (
        device_id   TEXT PRIMARY KEY,
        token_hash  BLOB NOT NULL UNIQUE,
        name        TEXT NOT NULL,
        kind        TEXT NOT NULL,
        scope       TEXT NOT NULL,
        created_at  TEXT NOT NULL,
        revoked_at  TEXT
    );

    CREATE TABLE drafts (
        session_id         TEXT PRIMARY KEY,
        text               TEXT NOT NULL,
        caret              INTEGER NOT NULL,
        updated_at_micros  INTEGER NOT NULL
    );

    CREATE TABLE access_journal (
        id         INTEGER PRIMARY KEY AUTOINCREMENT,
        at         TEXT NOT NULL,
        device_id  TEXT,
        action     TEXT NOT NULL,
        detail     TEXT
    );

    CREATE TABLE profiles (
        profile_id  TEXT PRIMARY KEY,
        data        TEXT NOT NULL
    );
    ",
    // v1 → v2: index drafts by recency so the stale-draft TTL purge (F09)
    // is an indexed range delete, not a full-table scan. Drafts have no
    // natural expiry (no FK, deleted only on explicit session close), so a
    // Control device looping PUTs under fresh ids would otherwise grow the
    // table without bound.
    "CREATE INDEX idx_drafts_updated_at ON drafts (updated_at_micros);",
];

/// Runs every pending migration. Idempotent.
pub fn migrate(conn: &mut Connection) -> Result<(), StoreError> {
    let current: usize = conn.query_row("PRAGMA user_version", [], |row| {
        row.get::<_, i64>(0)
            .map(|v| usize::try_from(v).unwrap_or(0))
    })?;

    for (index, script) in MIGRATIONS.iter().enumerate().skip(current) {
        let tx = conn.transaction()?;
        tx.execute_batch(script)?;
        // PRAGMA cannot be bound as a parameter; index comes from the
        // compile-time array, not from input.
        tx.execute_batch(&format!("PRAGMA user_version = {}", index + 1))?;
        tx.commit()?;
    }
    Ok(())
}

/// Current schema version expected by this build (the number of
/// migrations applied at a fully up-to-date `user_version`).
#[cfg(test)]
pub fn expected_version() -> usize {
    MIGRATIONS.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_and_are_idempotent() {
        let mut conn = Connection::open_in_memory().expect("open");
        migrate(&mut conn).expect("first run");
        migrate(&mut conn).expect("second run is a no-op");
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("version");
        assert_eq!(version, i64::try_from(expected_version()).expect("fits"));
        // The schema is actually usable.
        conn.execute(
            "INSERT INTO drafts (session_id, text, caret, updated_at_micros)
             VALUES ('s', 'hello', 5, 1)",
            [],
        )
        .expect("insert works");
    }
}
