// SPDX-License-Identifier: Apache-2.0

//! Master-key management (SPEC §9.A, D-9.1): 32 random bytes, stored in
//! the OS keystore (DPAPI / Secret Service) or — test and headless
//! fallback — in a file next to the database.
//!
//! The key is applied as a `SQLCipher` *raw key* (`PRAGMA key = "x'…'"`),
//! skipping the passphrase KDF: the entropy is already maximal and startup
//! stays fast (D-2.6: hub ready < 3 s).

use std::path::PathBuf;

use secrecy::{ExposeSecret, SecretString};

use crate::StoreError;

/// Where the master key lives.
#[derive(Debug, Clone)]
pub enum KeySource {
    /// OS keystore (production default — D-9.1).
    Keyring {
        /// Keystore service name (`fluence`).
        service: String,
        /// Keystore entry name (`store-key`).
        entry: String,
    },
    /// Hex key in a file (tests, headless installs without a keystore).
    /// Created with 0o600 permissions on Unix.
    File(PathBuf),
}

/// Loads the key, generating and persisting a fresh one on first run.
/// Returns the 64-hex-char representation.
pub fn load_or_create(source: &KeySource) -> Result<SecretString, StoreError> {
    match source {
        KeySource::Keyring { service, entry } => {
            let entry = keyring::Entry::new(service, entry)
                .map_err(|e| StoreError::Key(format!("keystore unavailable: {e}")))?;
            match entry.get_password() {
                Ok(existing) => validate_hex(&existing).map(|()| SecretString::from(existing)),
                Err(keyring::Error::NoEntry) => {
                    let fresh = generate_hex();
                    entry
                        .set_password(fresh.expose_secret())
                        .map_err(|e| StoreError::Key(format!("keystore write failed: {e}")))?;
                    Ok(fresh)
                }
                Err(e) => Err(StoreError::Key(format!("keystore read failed: {e}"))),
            }
        }
        KeySource::File(path) => {
            if path.exists() {
                let raw = std::fs::read_to_string(path)
                    .map_err(|e| StoreError::Key(format!("key file unreadable: {e}")))?;
                let trimmed = raw.trim().to_owned();
                validate_hex(&trimmed)?;
                Ok(SecretString::from(trimmed))
            } else {
                let fresh = generate_hex();
                write_restricted(path, fresh.expose_secret())?;
                Ok(fresh)
            }
        }
    }
}

/// 32 fresh random bytes as 64 hex chars.
fn generate_hex() -> SecretString {
    let bytes: [u8; 32] = rand::random();
    SecretString::from(hex::encode(bytes))
}

fn validate_hex(candidate: &str) -> Result<(), StoreError> {
    let valid = candidate.len() == 64 && candidate.chars().all(|c| c.is_ascii_hexdigit());
    if valid {
        Ok(())
    } else {
        Err(StoreError::Key("key material is not 64 hex chars".into()))
    }
}

/// Writes the key file with restrictive permissions (0o600 on Unix; on
/// Windows the profile directory ACL is the boundary).
fn write_restricted(path: &std::path::Path, contents: &str) -> Result<(), StoreError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| StoreError::Key(format!("cannot create key dir: {e}")))?;
    }
    std::fs::write(path, contents)
        .map_err(|e| StoreError::Key(format!("key write failed: {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| StoreError::Key(format!("key chmod failed: {e}")))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_key_is_created_then_reloaded_identically() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("store.key");
        let source = KeySource::File(path.clone());

        let first = load_or_create(&source).expect("create");
        let second = load_or_create(&source).expect("reload");
        assert_eq!(first.expose_secret(), second.expose_secret());
        assert_eq!(first.expose_secret().len(), 64);
    }

    #[test]
    fn corrupted_key_file_is_rejected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("store.key");
        std::fs::write(&path, "not-a-key").expect("write");
        let error = load_or_create(&KeySource::File(path)).expect_err("must reject");
        assert!(error.to_string().contains("64 hex"));
    }
}
