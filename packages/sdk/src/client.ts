// SPDX-License-Identifier: Apache-2.0

/**
 * `FluenceClient` — typed access to every hub route (SPEC §5.A).
 *
 * Zero business logic (PLAN task 1.4): URLs, token header, problem+json
 * errors, SSE/WS plumbing — nothing else. Anything smarter (debounce,
 * retries, reconnection) belongs to the applications or later SDK
 * versions.
 *
 * @packageDocumentation
 */

import { FluenceProblemError, problemFromResponse } from "./errors.js";
import { parseSseStream } from "./sse.js";
import type {
  CapabilitiesResponse,
  ConsentResponse,
  CreateMemoryItem,
  CreateSessionResponse,
  Draft,
  ForgetCandidates,
  ForgetRequest,
  HealthResponse,
  ListeningRequest,
  MemoryItem,
  MemorySearchResponse,
  NextCharsResponse,
  PairInfo,
  PairRequest,
  PairResponse,
  PendingResponse,
  Profile,
  SpeakRequest,
  SuggestEvent,
  SuggestRequest,
  TargetMap,
  Topic,
  Turn,
  VoicesResponse,
} from "./types.js";
import { openSocket, type FluenceSocket, type TopicHandlers } from "./ws.js";

/** Construction options. */
export interface FluenceClientOptions {
  /** Hub base URL (`http://127.0.0.1:7411` in embedded mode). */
  baseUrl: string;
  /** Device token (`X-Fluence-Token`). Only the pairing routes work
   * without one (SPEC §2.A). */
  token?: string;
  /** Injectable fetch (tests, custom TLS pinning layers). */
  fetch?: typeof globalThis.fetch;
}

/** HTTP methods used by the hub API. */
type Method = "GET" | "POST" | "PUT" | "DELETE";

/** Typed client for the Fluence hub. */
export class FluenceClient {
  readonly #baseUrl: string;
  readonly #token: string | undefined;
  readonly #fetch: typeof globalThis.fetch;

  constructor(options: FluenceClientOptions) {
    this.#baseUrl = options.baseUrl;
    this.#token = options.token;
    // Bind: an unbound fetch loses its globalThis receiver.
    this.#fetch = options.fetch ?? globalThis.fetch.bind(globalThis);
  }

  // ---- Pairing (SPEC §2.A) — the only tokenless routes ----

  /** Reads what a pairing screen needs to display. */
  async pairInfo(): Promise<PairInfo> {
    return this.#json("GET", "/pair/info");
  }

  /** Pairs this device (only while a pairing window is open). */
  async pair(request: PairRequest): Promise<PairResponse> {
    return this.#json("POST", "/pair", request);
  }

  // ---- Sessions (SPEC §5.A) ----

  /** Opens a conversation session (warm KV-cache hub-side). */
  async createSession(): Promise<CreateSessionResponse> {
    return this.#json("POST", "/api/v1/sessions");
  }

  /** Closes a session. */
  async deleteSession(sessionId: string): Promise<void> {
    await this.#noContent("DELETE", `/api/v1/sessions/${encodeURIComponent(sessionId)}`);
  }

  /** Ingests a conversation turn. */
  async addTurn(sessionId: string, turn: Turn): Promise<void> {
    await this.#noContent("POST", `/api/v1/sessions/${encodeURIComponent(sessionId)}/turns`, turn);
  }

  /** Synchronizes the draft (continuous autosave, D-2.6). */
  async putDraft(sessionId: string, draft: Draft): Promise<void> {
    await this.#noContent("PUT", `/api/v1/sessions/${encodeURIComponent(sessionId)}/draft`, draft);
  }

  /** Next-character distribution on the warm KV (adaptive dwell, Dasher). */
  async nextChars(sessionId: string, prefix: string): Promise<NextCharsResponse> {
    const path = `/api/v1/sessions/${encodeURIComponent(sessionId)}/next-chars`;
    return this.#json("GET", `${path}?prefix=${encodeURIComponent(prefix)}`);
  }

  /**
   * Streams suggestions over SSE. A new request on the same slot makes the
   * previous stream end with an `aborted` event (server-side cancellation,
   * SPEC §5.A).
   */
  async *suggest(
    sessionId: string,
    request: SuggestRequest,
    signal?: AbortSignal,
  ): AsyncGenerator<SuggestEvent, void, undefined> {
    const path = `/api/v1/sessions/${encodeURIComponent(sessionId)}/suggest`;
    const response = await this.#request("POST", path, request, signal);
    if (response.body === null) {
      throw new Error("suggest: response has no body stream");
    }
    for await (const raw of parseSseStream(response.body)) {
      // SSE wire → canonical form: event name is the tag, data the payload.
      // Transport-level SDK: the hub's shape is trusted, no runtime
      // validation in v0.
      const data: unknown = JSON.parse(raw.data);
      yield { event: raw.event, data } as SuggestEvent;
    }
  }

  // ---- Input (SPEC §4.A) ----

  /** Declares the full target map of a surface. */
  async putTargets(map: TargetMap): Promise<void> {
    await this.#noContent("PUT", "/api/v1/input/targets", map);
  }

  // ---- Voice (SPEC §6) ----

  /**
   * Vocalizes text. Returns the raw streamed-audio response
   * (`audio/ogg; codecs=opus`) — the caller consumes `response.body`.
   */
  async speak(request: SpeakRequest, signal?: AbortSignal): Promise<Response> {
    return this.#request("POST", "/api/v1/voice/speak", request, signal);
  }

  /** Lists installed voices. */
  async voices(): Promise<VoicesResponse> {
    return this.#json("GET", "/api/v1/voice/voices");
  }

  // ---- System (SPEC §2.C) ----

  /** Worker states, models, rolling latencies. */
  async health(): Promise<HealthResponse> {
    return this.#json("GET", "/api/v1/system/health");
  }

  /** Installation tier and available features. */
  async capabilities(): Promise<CapabilitiesResponse> {
    return this.#json("GET", "/api/v1/system/capabilities");
  }

  // ---- Memory (SPEC §5.B) — @experimental: may change until Phase 9 ----

  /** Creates a memory item. @experimental */
  async createMemoryItem(item: CreateMemoryItem): Promise<MemoryItem> {
    return this.#json("POST", "/api/v1/memory/items", item);
  }

  /** Searches personal memory (ACL-filtered). @experimental */
  async searchMemory(query: string): Promise<MemorySearchResponse> {
    return this.#json("GET", `/api/v1/memory/search?q=${encodeURIComponent(query)}`);
  }

  /** Deletes a memory item. @experimental */
  async deleteMemoryItem(itemId: string): Promise<void> {
    await this.#noContent("DELETE", `/api/v1/memory/items/${encodeURIComponent(itemId)}`);
  }

  /** Reads the learned-candidate validation queue. @experimental */
  async memoryPending(): Promise<PendingResponse> {
    return this.#json("GET", "/api/v1/memory/pending");
  }

  /** Accepts a learned candidate into memory. @experimental */
  async acceptPendingMemory(itemId: string): Promise<void> {
    await this.#noContent("POST", `/api/v1/memory/pending/${encodeURIComponent(itemId)}/accept`);
  }

  /** Rejects a learned candidate. @experimental */
  async rejectPendingMemory(itemId: string): Promise<void> {
    await this.#noContent("POST", `/api/v1/memory/pending/${encodeURIComponent(itemId)}/reject`);
  }

  /** Semantic forgetting: lists candidates to confirm. @experimental */
  async forgetMemory(request: ForgetRequest): Promise<ForgetCandidates> {
    return this.#json("POST", "/api/v1/memory/forget", request);
  }

  // ---- ASR consent (SPEC §5.A) — @experimental ----

  /** Obtains a journaled consent token. @experimental */
  async asrConsent(): Promise<ConsentResponse> {
    return this.#json("POST", "/api/v1/asr/consent");
  }

  /** Starts/stops partner-speech listening. @experimental */
  async setAsrListening(request: ListeningRequest): Promise<void> {
    await this.#noContent("POST", "/api/v1/asr/listening", request);
  }

  // ---- Profiles (SPEC §7.B) — @experimental ----

  /** Reads a profile. @experimental */
  async getProfile(profileId: string): Promise<Profile> {
    return this.#json("GET", `/api/v1/profiles/${encodeURIComponent(profileId)}`);
  }

  /** Replaces a profile. @experimental */
  async putProfile(profile: Profile): Promise<void> {
    await this.#noContent("PUT", `/api/v1/profiles/${encodeURIComponent(profile.id)}`, profile);
  }

  // ---- WebSocket (SPEC §2.A) ----

  /** Opens the multiplexed hub socket on the given topics. */
  socket(topics: Topic[], handlers: TopicHandlers): FluenceSocket {
    return openSocket({
      baseUrl: this.#baseUrl,
      topics,
      token: this.#token,
      handlers,
    });
  }

  // ---- Transport plumbing ----

  /** Sends a request; throws [`FluenceProblemError`] on non-2xx. */
  async #request(
    method: Method,
    path: string,
    body?: unknown,
    signal?: AbortSignal,
  ): Promise<Response> {
    const headers: Record<string, string> = {};
    if (this.#token !== undefined) {
      headers["X-Fluence-Token"] = this.#token;
    }
    if (body !== undefined) {
      headers["Content-Type"] = "application/json";
    }
    const response = await this.#fetch(new URL(path, this.#baseUrl), {
      method,
      headers,
      body: body === undefined ? null : JSON.stringify(body),
      signal: signal ?? null,
    });
    if (!response.ok) {
      throw await problemFromResponse(response);
    }
    return response;
  }

  async #json<T>(method: Method, path: string, body?: unknown): Promise<T> {
    const response = await this.#request(method, path, body);
    return (await response.json()) as T;
  }

  async #noContent(method: Method, path: string, body?: unknown): Promise<void> {
    await this.#request(method, path, body);
  }
}

export { FluenceProblemError };
