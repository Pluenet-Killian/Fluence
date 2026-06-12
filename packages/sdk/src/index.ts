// SPDX-License-Identifier: Apache-2.0

/**
 * `@fluence/sdk` — typed client for the Fluence hub API (SPEC §2.B, D-2.5).
 *
 * Will combine types generated from `fluence-protocol` (the single source of
 * truth — JSON Schema → OpenAPI 3.1 → `src/generated/`) with a thin ergonomic
 * layer: fetch + SSE for generations, WebSocket for events, zero business
 * logic. CI fails when generated artifacts drift from the Rust definitions
 * (`cargo xtask check-contracts`).
 *
 * PLAN Phase 1 populates this package (task 1.4). Until then it exports
 * nothing.
 *
 * @packageDocumentation
 */

export {};
