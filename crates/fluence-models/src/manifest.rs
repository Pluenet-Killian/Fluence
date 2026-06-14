// SPDX-License-Identifier: Apache-2.0

//! The pinned model manifest (`models/*.json`) — the single source of truth for
//! its shape and validation, shared by the provisioning xtask and (later) the
//! installer/hub.

use std::collections::BTreeSet;

use serde::Deserialize;

/// The only manifest schema version this build understands.
pub const SUPPORTED_VERSION: u32 = 1;

/// A pinned model manifest.
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    /// Schema version (only [`SUPPORTED_VERSION`] is understood).
    pub version: u32,
    /// The models the manifest pins.
    pub models: Vec<ModelEntry>,
}

/// One pinned model.
#[derive(Debug, Clone, Deserialize)]
pub struct ModelEntry {
    /// Stable identifier for logs.
    pub id: String,
    /// Cache filename (a plain name, no path separators).
    pub file: String,
    /// Source URL (HTTPS) — a hint; the content is verified, not trusted.
    pub url: String,
    /// Expected SHA-256, lowercase hex — the per-file integrity contract.
    pub sha256: String,
    /// Expected size in bytes (a cheap completeness check before hashing).
    pub bytes: u64,
    /// SPDX license id (compliance reporting); optional.
    #[serde(default)]
    pub license: Option<String>,
    /// Human note on what the model is for; optional.
    #[serde(default)]
    pub purpose: Option<String>,
}

/// Why a manifest was rejected — loud and precise, never silent.
#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    /// The bytes are not valid JSON for the manifest shape.
    #[error("manifest is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// Unknown schema version.
    #[error("unsupported manifest version {0} (this build understands {SUPPORTED_VERSION})")]
    Version(u32),
    /// A manifest with no models is almost certainly a mistake.
    #[error("manifest lists no models")]
    Empty,
    /// `sha256` is not 64 hex characters.
    #[error("model {id}: sha256 must be 64 hex characters")]
    Sha256 {
        /// Offending model id.
        id: String,
    },
    /// `url` is not HTTPS.
    #[error("model {id}: url must be https")]
    Url {
        /// Offending model id.
        id: String,
    },
    /// `file` is not a plain filename (empty, separators, or `..`).
    #[error("model {id}: file must be a plain filename, got {file:?}")]
    FileName {
        /// Offending model id.
        id: String,
        /// The rejected filename.
        file: String,
    },
}

impl Manifest {
    /// Parses and validates a manifest from JSON bytes.
    ///
    /// # Errors
    ///
    /// [`ManifestError`] when the JSON is malformed or a field is invalid.
    pub fn parse(bytes: &[u8]) -> Result<Self, ManifestError> {
        let manifest: Self = serde_json::from_slice(bytes)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Validates a parsed manifest (version, non-empty, per-model fields).
    ///
    /// # Errors
    ///
    /// [`ManifestError`] for the first problem found.
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.version != SUPPORTED_VERSION {
            return Err(ManifestError::Version(self.version));
        }
        if self.models.is_empty() {
            return Err(ManifestError::Empty);
        }
        for model in &self.models {
            if model.sha256.len() != 64 || !model.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(ManifestError::Sha256 {
                    id: model.id.clone(),
                });
            }
            if !model.url.starts_with("https://") {
                return Err(ManifestError::Url {
                    id: model.id.clone(),
                });
            }
            // The filename is joined onto a cache directory: reject anything that
            // could escape it (path traversal) or denote a directory.
            if model.file.is_empty()
                || model.file.contains('/')
                || model.file.contains('\\')
                || model.file.contains("..")
            {
                return Err(ManifestError::FileName {
                    id: model.id.clone(),
                    file: model.file.clone(),
                });
            }
        }
        Ok(())
    }

    /// The set of filenames this manifest references — what GC must keep.
    #[must_use]
    pub fn referenced_files(&self) -> BTreeSet<String> {
        self.models.iter().map(|m| m.file.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{
        "version": 1,
        "models": [
            {"id":"tiny","file":"tiny.gguf","url":"https://example/m",
             "sha256":"ed5fa30c487b282ec156c29062f1222e5c20875a944ac98289dbd242e947f747",
             "bytes":105454144,"license":"Apache-2.0","purpose":"mechanics"}
        ]
    }"#;

    #[test]
    fn parses_a_valid_manifest_and_lists_referenced_files() {
        let manifest = Manifest::parse(GOOD.as_bytes()).expect("valid");
        assert_eq!(manifest.version, 1);
        assert_eq!(manifest.models[0].license.as_deref(), Some("Apache-2.0"));
        assert!(manifest.referenced_files().contains("tiny.gguf"));
    }

    #[test]
    fn optional_fields_default_to_none() {
        let bare = r#"{"version":1,"models":[{"id":"m","file":"m.gguf",
            "url":"https://e/m","sha256":"a0b1c2d3e4f5061728394a5b6c7d8e9f00112233445566778899aabbccddeeff",
            "bytes":1}]}"#;
        let manifest = Manifest::parse(bare.as_bytes()).expect("valid");
        assert!(manifest.models[0].license.is_none());
        assert!(manifest.models[0].purpose.is_none());
    }

    #[test]
    fn rejects_version_empty_sha_url_and_traversal() {
        let v2 = GOOD.replace("\"version\": 1", "\"version\": 2");
        assert!(matches!(
            Manifest::parse(v2.as_bytes()),
            Err(ManifestError::Version(2))
        ));

        let empty = r#"{"version":1,"models":[]}"#;
        assert!(matches!(
            Manifest::parse(empty.as_bytes()),
            Err(ManifestError::Empty)
        ));

        let bad_sha = GOOD.replace(
            "ed5fa30c487b282ec156c29062f1222e5c20875a944ac98289dbd242e947f747",
            "xyz",
        );
        assert!(matches!(
            Manifest::parse(bad_sha.as_bytes()),
            Err(ManifestError::Sha256 { .. })
        ));

        let http = GOOD.replace("https://example/m", "http://insecure/m");
        assert!(matches!(
            Manifest::parse(http.as_bytes()),
            Err(ManifestError::Url { .. })
        ));

        let traversal = GOOD.replace("tiny.gguf", "../escape");
        assert!(matches!(
            Manifest::parse(traversal.as_bytes()),
            Err(ManifestError::FileName { .. })
        ));
    }
}
