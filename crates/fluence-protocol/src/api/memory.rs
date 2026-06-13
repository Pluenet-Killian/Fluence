// SPDX-License-Identifier: Apache-2.0

//! Personal memory subsystem (SPEC §5.B, D-5.6).
//!
//! Everything here is **P0 intimate data**: encrypted at rest, never leaves
//! the household, never appears in logs (SPEC §9.A). Learned items go
//! through a validation queue — nothing enters learned memory without the
//! user's confirmation (agency first).
//!
//! Stability: **experimental** (P2 domain, PLAN task 1.3bis) — these types
//! may still change while the memory subsystem is built (Phase 9); enums
//! are `non_exhaustive`, response shapes may gain fields.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::common::{MemoryItemId, Normalized};

/// Kind of memory item (SPEC §5.B data model).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum MemoryKind {
    /// A person (« Marie, ma fille — vient le mardi »).
    Person,
    /// A place.
    Place,
    /// A recurring routine.
    Routine,
    /// A dated anecdote.
    Anecdote,
    /// A recurring phrase — also a zero-latency suggestion cache (§5.B).
    Phrase,
    /// A standalone fact.
    Fact,
    /// A summary persisted after raw-conversation purge (30 days).
    ConversationSummary,
}

/// How an item entered memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum MemorySource {
    /// Entered by the user (or caregiver per ACL) in « Ma mémoire ».
    Manual,
    /// Extracted by the post-conversation job, then user-confirmed.
    Learned,
    /// Imported from contacts (vCard/CSV) or calendar (ICS), locally.
    Imported,
}

/// Who may see/edit an item (SPEC §5.B). The caregiver space **never**
/// sees `private` items, even as admin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum MemoryAcl {
    /// Visible to the user only (the default — intimacy first).
    Private,
    /// Caregivers may see it.
    CareVisible,
    /// Caregivers may see and edit it.
    CareEditable,
}

impl Default for MemoryAcl {
    fn default() -> Self {
        Self::Private
    }
}

/// A memory item (SPEC §5.B data model).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryItem {
    /// Stable id.
    pub id: MemoryItemId,
    /// Item kind.
    pub kind: MemoryKind,
    /// The remembered content. **P0.**
    pub content: String,
    /// Free-form tags (`famille`…).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// How it entered memory.
    pub source: MemorySource,
    /// Access control (default `private`).
    #[serde(default)]
    pub acl: MemoryAcl,
    /// Creation time.
    pub created_at: DateTime<Utc>,
    /// Last retrieval-injection time, if ever used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<DateTime<Utc>>,
    /// Retrieval-injection count (drives recency/frequency boost).
    pub use_count: u32,
    /// Extraction confidence (1.0 for manual entries).
    pub confidence: Normalized,
}

/// `POST /memory/items` request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CreateMemoryItem {
    /// Item kind.
    pub kind: MemoryKind,
    /// The content to remember. **P0.**
    pub content: String,
    /// Free-form tags.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Access control (default `private`).
    #[serde(default)]
    pub acl: MemoryAcl,
}

/// `GET /memory/search?q=` response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemorySearchResponse {
    /// Matching items, best first (hybrid BM25 + vector, RRF fusion §5.B),
    /// filtered by the caller's ACL.
    pub items: Vec<MemoryItem>,
}

/// `GET /memory/pending` response — the validation queue (SPEC §5.B):
/// candidates extracted after conversations, awaiting accept/reject.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PendingResponse {
    /// Candidates awaiting user decision.
    pub items: Vec<PendingMemoryItem>,
}

/// A candidate item in the validation queue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PendingMemoryItem {
    /// Queue id (used in `POST /memory/pending/{id}/accept|reject`).
    pub id: MemoryItemId,
    /// Proposed kind.
    pub kind: MemoryKind,
    /// Proposed content. **P0.**
    pub content: String,
    /// Proposed tags.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Why it was proposed (« phrase répétée 3× cette semaine »). Shown to
    /// the user; stored encrypted like everything else. **P0.**
    pub reason: String,
    /// Extraction confidence.
    pub confidence: Normalized,
    /// When the extraction job proposed it.
    pub proposed_at: DateTime<Utc>,
}

/// `POST /memory/forget` request — semantic forgetting (SPEC §5.B).
///
/// Two-step flow: this returns candidate items; actual deletion goes
/// through `DELETE /memory/items/{id}` after user confirmation. The
/// forget journal records **metadata only**, never the forgotten content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ForgetRequest {
    /// What to forget, in the user's words (« tout ce qui concerne X »).
    /// **P0.**
    pub about: String,
}

/// `POST /memory/forget` response: candidates to confirm.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ForgetCandidates {
    /// Items the semantic search proposes to forget.
    pub items: Vec<MemoryItem>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acl_defaults_to_private() {
        // SPEC §5.B: intimacy is the default, sharing is the choice.
        let item: CreateMemoryItem = serde_json::from_str(
            r#"{"kind":"person","content":"Marie, ma fille — vient le mardi"}"#,
        )
        .unwrap();
        assert_eq!(item.acl, MemoryAcl::Private);
        assert!(item.tags.is_empty());
    }

    #[test]
    fn memory_item_matches_spec_example_shape() {
        // SPEC §5.B wire example, adapted to the typed fields.
        let json = r#"{
            "id":"m_1","kind":"person",
            "content":"Marie, ma fille — vient le mardi, deux enfants (Léo, Zoé)",
            "tags":["famille"],"source":"manual","acl":"private",
            "created_at":"2026-06-13T10:00:00Z","use_count":17,"confidence":0.9
        }"#;
        let item: MemoryItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.kind, MemoryKind::Person);
        assert_eq!(item.use_count, 17);
        assert!(item.last_used_at.is_none());
    }
}
