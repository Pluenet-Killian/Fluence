// SPDX-License-Identifier: Apache-2.0

//! The recovery secret behind the printable backup kit (PLAN 7.3, SPEC §9.A).
//!
//! A backup archive is a fully independent `SQLCipher` database — the same
//! audited AES-256 as the live store — but keyed by a freshly generated
//! 256-bit secret instead of the machine keystore. That secret is what lets a
//! backup restore on a **different** machine (new install, replaced disk,
//! reinstalled OS): the live store key never leaves the OS keystore, while the
//! recovery secret leaves on paper, as the printable kit.
//!
//! Encoding: the 32 secret bytes plus a 2-byte SHA-256 checksum are rendered
//! as grouped uppercase hex — unambiguous (`0`-`9`, `A`-`F`), trivially
//! auditable, and the checksum catches a transcription slip *before* a failed
//! restore wastes the operator's time. The CLI renders the same string as a QR
//! code, so there is a single parser and a single checksum check whether the
//! kit is scanned or typed. Possession of the phrase is exactly the capability
//! to decrypt the backup — there is no separate password (the entropy is
//! already maximal, so no KDF, mirroring the store's raw-key choice, D-2.6).

use secrecy::SecretString;
use sha2::{Digest, Sha256};

/// Bytes of entropy in a recovery secret (256-bit, matching the `SQLCipher`
/// raw key the live store uses).
const SECRET_LEN: usize = 32;
/// SHA-256 prefix appended as a transcription checksum.
const CHECKSUM_LEN: usize = 2;
/// Hex characters per printed group (`7F3A-91C2-…`).
const GROUP: usize = 4;

/// Why a recovery phrase could not be parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RecoveryError {
    /// Not valid hex of the expected length once separators are stripped.
    #[error("recovery phrase is malformed (expected grouped hex of the recovery secret)")]
    Malformed,
    /// Decoded, but the checksum disagrees — almost certainly a typo.
    #[error("recovery phrase checksum does not match (likely a transcription error)")]
    Checksum,
}

/// A 256-bit backup secret. Never logged: its `Debug` is redacted and the raw
/// bytes are only ever exposed as a `SQLCipher` key inside this crate.
#[derive(Clone)]
pub struct RecoverySecret {
    bytes: [u8; SECRET_LEN],
}

impl std::fmt::Debug for RecoverySecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Redacted on purpose — the secret must never reach a log or panic.
        f.debug_struct("RecoverySecret").finish_non_exhaustive()
    }
}

impl RecoverySecret {
    /// Generates a fresh secret from the OS CSPRNG.
    #[must_use]
    pub fn generate() -> Self {
        Self {
            bytes: rand::random(),
        }
    }

    /// Reconstructs a secret from its raw bytes (the QR payload path / tests).
    #[must_use]
    pub fn from_bytes(bytes: [u8; SECRET_LEN]) -> Self {
        Self { bytes }
    }

    /// The printable phrase: grouped uppercase hex of the secret and its
    /// checksum (`7F3A-91C2-…-AB12`).
    #[must_use]
    pub fn phrase(&self) -> String {
        let mut payload = Vec::with_capacity(SECRET_LEN + CHECKSUM_LEN);
        payload.extend_from_slice(&self.bytes);
        payload.extend_from_slice(&self.checksum());
        group_hex(&hex::encode_upper(&payload))
    }

    /// Parses a phrase, tolerating spaces, dashes and case, and verifying the
    /// checksum.
    ///
    /// # Errors
    ///
    /// [`RecoveryError::Malformed`] when the input is not hex of the right
    /// length; [`RecoveryError::Checksum`] when the checksum disagrees.
    pub fn from_phrase(input: &str) -> Result<Self, RecoveryError> {
        // Keep only hex digits: drops the grouping dashes, spaces, and any
        // stray separator the operator may have added, in any case.
        let cleaned: String = input.chars().filter(char::is_ascii_hexdigit).collect();
        let decoded = hex::decode(&cleaned).map_err(|_| RecoveryError::Malformed)?;
        if decoded.len() != SECRET_LEN + CHECKSUM_LEN {
            return Err(RecoveryError::Malformed);
        }
        let (secret, checksum) = decoded.split_at(SECRET_LEN);
        let mut bytes = [0u8; SECRET_LEN];
        bytes.copy_from_slice(secret);
        let candidate = Self { bytes };
        if checksum == candidate.checksum() {
            Ok(candidate)
        } else {
            Err(RecoveryError::Checksum)
        }
    }

    /// The `SQLCipher` raw key (64 lowercase hex chars) this secret encrypts
    /// backups with. Crate-private: callers go through `back_up`/`restore`.
    pub(crate) fn key_hex(&self) -> SecretString {
        SecretString::from(hex::encode(self.bytes))
    }

    fn checksum(&self) -> [u8; CHECKSUM_LEN] {
        let digest = Sha256::digest(self.bytes);
        let mut out = [0u8; CHECKSUM_LEN];
        out.copy_from_slice(&digest[..CHECKSUM_LEN]);
        out
    }
}

/// Inserts a dash every [`GROUP`] characters for legibility.
fn group_hex(hex: &str) -> String {
    let mut out = String::with_capacity(hex.len() + hex.len() / GROUP);
    for (i, c) in hex.chars().enumerate() {
        if i != 0 && i % GROUP == 0 {
            out.push('-');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    fn key(secret: &RecoverySecret) -> String {
        secret.key_hex().expose_secret().to_owned()
    }

    #[test]
    fn phrase_round_trips() {
        let secret = RecoverySecret::from_bytes([0xAB; 32]);
        let back = RecoverySecret::from_phrase(&secret.phrase()).expect("parse");
        assert_eq!(key(&back), key(&secret));
    }

    #[test]
    fn phrase_is_grouped_uppercase_hex() {
        let phrase = RecoverySecret::from_bytes([0x0F; 32]).phrase();
        assert!(phrase.contains('-'), "groups are dash-separated");
        assert!(
            phrase.chars().all(|c| c.is_ascii_hexdigit() || c == '-'),
            "only hex and dashes"
        );
        assert_eq!(phrase.to_uppercase(), phrase, "rendered uppercase");
    }

    #[test]
    fn tolerates_spaces_dashes_and_lowercase() {
        let secret = RecoverySecret::from_bytes([0x12; 32]);
        // How a careful human might re-type it: lowercase, spaces not dashes.
        let typed = secret.phrase().to_lowercase().replace('-', "  ");
        let back = RecoverySecret::from_phrase(&typed).expect("parse");
        assert_eq!(key(&back), key(&secret));
    }

    #[test]
    fn a_flipped_character_fails_the_checksum() {
        let phrase = RecoverySecret::from_bytes([0x01; 32]).phrase();
        let mut chars: Vec<char> = phrase.chars().collect();
        // Deterministically flip the first hex digit (no RNG in tests).
        chars[0] = if chars[0] == '0' { '1' } else { '0' };
        let tampered: String = chars.into_iter().collect();
        // `unwrap_err` only needs the (redacted) `Debug`; the secret is never
        // `PartialEq`, so it can't be compared in non-constant time by mistake.
        assert_eq!(
            RecoverySecret::from_phrase(&tampered).unwrap_err(),
            RecoveryError::Checksum
        );
    }

    #[test]
    fn malformed_input_is_rejected() {
        assert_eq!(
            RecoverySecret::from_phrase("xyz").unwrap_err(),
            RecoveryError::Malformed
        );
        assert_eq!(
            RecoverySecret::from_phrase("").unwrap_err(),
            RecoveryError::Malformed
        );
        // Right alphabet, wrong length.
        assert_eq!(
            RecoverySecret::from_phrase("DEAD-BEEF").unwrap_err(),
            RecoveryError::Malformed
        );
    }

    #[test]
    fn debug_never_leaks_the_secret() {
        let secret = RecoverySecret::from_bytes([0x42; 32]);
        let shown = format!("{secret:?}");
        assert!(!shown.contains("42"), "redacted Debug: {shown}");
        assert!(!shown.contains(&hex::encode([0x42u8; 32])));
    }

    #[test]
    fn generated_secrets_differ() {
        assert_ne!(
            key(&RecoverySecret::generate()),
            key(&RecoverySecret::generate())
        );
    }
}
