// SPDX-License-Identifier: Apache-2.0

/**
 * `@fluence/sdk` — typed client for the Fluence hub API (SPEC §2.B, D-2.5).
 *
 * Generated types (`cargo xtask check-contracts`, drift-checked in CI) plus
 * a thin ergonomic layer: fetch + SSE for generations, WebSocket for
 * events, zero business logic.
 *
 * ```ts
 * const client = new FluenceClient({ baseUrl: "http://127.0.0.1:7411", token });
 * const { session_id } = await client.createSession();
 * for await (const event of client.suggest(session_id, {
 *   mode: "rephrase", draft: "veu eau frache ce soir", n: 3, slot: "main",
 * })) {
 *   if (event.event === "final") show(event.data.suggestions);
 * }
 * ```
 *
 * @packageDocumentation
 */

export { FluenceClient, FluenceProblemError, type FluenceClientOptions } from "./client.js";
export { buildWsUrl, openSocket } from "./ws.js";
export type { FluenceSocket, SocketOptions, TopicHandlers } from "./ws.js";
export { parseSseStream, type RawSseEvent } from "./sse.js";
export type * from "./types.js";
