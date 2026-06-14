// SPDX-License-Identifier: Apache-2.0

//! Model manifest integrity and lifecycle (D-3.2; PLAN 7.4).
//!
//! A manifest pins each model by URL + **SHA-256** + size; the hash is the
//! per-file integrity contract. On top of that, the manifest itself can carry a
//! **minisign signature** so a provisioning step can prove it came from the
//! project's release key, not just from whoever served the file — the
//! supply-chain link the in-repo, git-trusted manifest does not need but a
//! downloaded model pack does.
//!
//! This crate is the single source of truth for the manifest shape and its
//! verification, shared by the provisioning xtask today and the desktop
//! installer/hub tomorrow. Signing (producing the `.minisig`) is an operator
//! step done with the `minisign` tool and the release **private** key — a
//! credential, never in this repo; here we only ever *verify*.

mod gc;
mod manifest;
mod verify;

pub use gc::{GcPlan, plan_gc};
pub use manifest::{Manifest, ManifestError, ModelEntry, SUPPORTED_VERSION};
pub use verify::{VerifyError, verify_manifest_signature, verify_sha256};
