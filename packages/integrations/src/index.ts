// SPDX-License-Identifier: Apache-2.0

/**
 * `@fluence/integrations` — ecosystem adapters (SPEC §2.B, §10).
 *
 * Will host `asterics-grid-plugin` (external prediction source backed by the
 * hub API, D-10.3) and `dasher-lm` (a language model for Dasher v6 built on
 * `next-chars`, §5.A). Strategy: capture the ecosystem by feeding it —
 * every adapter consumes the same public API as our own clients.
 *
 * Work starts when the hub API is demonstrable (PLAN: parallel track after
 * Phase 4; deliverables in P2, D-10.2). Until then this package exports
 * nothing.
 *
 * @packageDocumentation
 */

export {};
