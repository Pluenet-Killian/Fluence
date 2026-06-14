// SPDX-License-Identifier: Apache-2.0

//! Signature and hash verification (D-3.2).
//!
//! Two independent integrity layers: a **minisign** signature proves the
//! manifest came from the project's release key (supply chain), and a
//! **SHA-256** per file proves each downloaded blob is the pinned content. Both
//! fail **closed** — a mismatch is a hard error, never a silently-trusted file.

use minisign_verify::{PublicKey, Signature};
use sha2::{Digest, Sha256};

/// Why verification failed — distinct variants so the caller can tell a
/// malformed input apart from a genuine signature/hash mismatch.
#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    /// The trusted public key string is not a valid minisign key.
    #[error("trusted public key is malformed: {0}")]
    PublicKey(String),
    /// The signature blob is not a valid minisign signature.
    #[error("signature is malformed: {0}")]
    Signature(String),
    /// The signature is well-formed but does not verify against the key — the
    /// content was tampered with, or signed by a different key.
    #[error("signature does not verify against the trusted key")]
    Mismatch,
    /// A blob's SHA-256 does not match the manifest's pin.
    #[error("sha256 mismatch: expected {expected}, got {actual}")]
    Sha256 {
        /// The pinned hash.
        expected: String,
        /// The hash actually computed.
        actual: String,
    },
}

/// Verifies that `manifest_bytes` carries a valid minisign signature `sig` from
/// the holder of `public_key_b64` (the project's release key).
///
/// Accepts both minisign modes — modern pre-hashed (`ED`) and legacy (`Ed`,
/// ed25519 over the raw bytes). We do not control which `minisign` version the
/// operator signs with, and both are EUF-CMA secure with no downgrade path
/// (forging either still needs the private key). The signature binds the exact
/// bytes: any change to the manifest invalidates it.
///
/// # Errors
///
/// [`VerifyError::PublicKey`]/[`VerifyError::Signature`] when an input is
/// malformed; [`VerifyError::Mismatch`] when the signature does not verify.
pub fn verify_manifest_signature(
    manifest_bytes: &[u8],
    sig: &str,
    public_key_b64: &str,
) -> Result<(), VerifyError> {
    let public_key = PublicKey::from_base64(public_key_b64)
        .map_err(|e| VerifyError::PublicKey(e.to_string()))?;
    let signature = Signature::decode(sig).map_err(|e| VerifyError::Signature(e.to_string()))?;
    public_key
        .verify(manifest_bytes, &signature, true)
        .map_err(|_| VerifyError::Mismatch)
}

/// Verifies a blob against an expected lowercase-hex SHA-256 (the per-model
/// integrity contract). Case-insensitive on the expected hex.
///
/// # Errors
///
/// [`VerifyError::Sha256`] when the hash does not match.
pub fn verify_sha256(data: &[u8], expected_hex: &str) -> Result<(), VerifyError> {
    let actual = hex_lower(&Sha256::digest(data));
    if actual.eq_ignore_ascii_case(expected_hex) {
        Ok(())
    } else {
        Err(VerifyError::Sha256 {
            expected: expected_hex.to_owned(),
            actual,
        })
    }
}

/// Lowercase-hex encoding of a byte slice.
fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // The official minisign test vector (github.com/jedisct1/rust-minisign-verify):
    // a real Ed25519 signature over the bytes `b"test"`. Using a genuine vector
    // proves our wiring against the audited verifier without holding any private
    // key — signing stays an operator credential (D-3.2).
    const VECTOR_PK: &str = "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    const VECTOR_SIG: &str = "untrusted comment: signature from minisign secret key\nRWQf6LRCGA9i59SLOFxz6NxvASXDJeRtuZykwQepbDEGt87ig1BNpWaVWuNrm73YiIiJbq71Wi+dP9eKL8OC351vwIasSSbXxwA=\ntrusted comment: timestamp:1555779966\tfile:test\nQtKMXWyYcwdpZAlPF7tE2ENJkRd1ujvKjlj1m9RtHTBnZPa5WKU5uWRs5GoP5M/VqE81QFuMKI5k/SfNQUaOAA==";
    const VECTOR_MSG: &[u8] = b"test";

    #[test]
    fn the_official_minisign_vector_verifies() {
        verify_manifest_signature(VECTOR_MSG, VECTOR_SIG, VECTOR_PK)
            .expect("a real minisign signature must verify");
    }

    #[test]
    fn a_tampered_message_is_rejected() {
        // The signature binds the content: change one byte and it must fail.
        let err = verify_manifest_signature(b"tEst", VECTOR_SIG, VECTOR_PK).expect_err("must fail");
        assert!(matches!(err, VerifyError::Mismatch), "got: {err}");
    }

    #[test]
    fn a_malformed_public_key_is_an_error() {
        let err = verify_manifest_signature(VECTOR_MSG, VECTOR_SIG, "not-a-key").expect_err("fail");
        assert!(matches!(err, VerifyError::PublicKey(_)), "got: {err}");
    }

    #[test]
    fn a_malformed_signature_is_an_error() {
        let err =
            verify_manifest_signature(VECTOR_MSG, "garbage", VECTOR_PK).expect_err("must fail");
        assert!(matches!(err, VerifyError::Signature(_)), "got: {err}");
    }

    #[test]
    fn sha256_matches_and_mismatches() {
        // SHA-256("test") — the same bytes as the vector message.
        let expected = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
        verify_sha256(b"test", expected).expect("matching hash");
        verify_sha256(b"test", &expected.to_uppercase()).expect("case-insensitive");
        let err = verify_sha256(b"tampered", expected).expect_err("must fail");
        assert!(matches!(err, VerifyError::Sha256 { .. }), "got: {err}");
    }
}
