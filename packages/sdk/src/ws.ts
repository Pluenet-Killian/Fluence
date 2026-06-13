// SPDX-License-Identifier: Apache-2.0

/**
 * Typed wrapper over the hub WebSocket (SPEC §2.A): one `/ws` connection,
 * topics negotiated at open via the query string, frames dispatched by
 * topic.
 *
 * v0 scope (PLAN task 1.4: zero business logic): connect, subscribe at
 * open, typed dispatch, close. Automatic reconnection with session
 * resumption arrives with the composer (Phase 5).
 *
 * @packageDocumentation
 */

import type { SelectionEvent, ServerFrame, SystemEvent, Topic } from "./types.js";

// (Re-exported for handler signatures; the dispatch below relies on the
// generated discriminated union to narrow them.)
export type { SelectionEvent, SystemEvent };

/** Handlers for the topics carrying payloads in contract v1. */
export interface TopicHandlers {
  /** Selection engine events (`input` topic). */
  input?: (event: SelectionEvent) => void;
  /** System state events (`system` topic). */
  system?: (event: SystemEvent) => void;
  /** Called for frames whose topic has no handler (forward compatibility). */
  unknown?: (frame: unknown) => void;
}

/** Options for [`openSocket`]. */
export interface SocketOptions {
  /** Hub base URL (`http://127.0.0.1:7411`). */
  baseUrl: string;
  /** Topics to subscribe to (filtered by scope hub-side). */
  topics: Topic[];
  /** Device token. The browser `WebSocket` API cannot set headers, so it
   * travels as a query parameter (loopback or TLS — see ADR-0004).
   * (`| undefined`: callers forward their own optional token.) */
  token?: string | undefined;
  /** Input protocol version (defaults to 1). */
  v?: number | undefined;
  /** Frame handlers. */
  handlers: TopicHandlers;
  /** Injectable WebSocket constructor (tests). */
  webSocketCtor?: new (url: string) => WebSocket;
}

/**
 * Builds the `/ws` URL: http(s) base → ws(s), topics/version/token in the
 * query string (SPEC §4.A: `v` negotiated at open).
 */
export function buildWsUrl(baseUrl: string, topics: Topic[], v: number, token?: string): string {
  const url = new URL("/ws", baseUrl);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  url.searchParams.set("topics", topics.join(","));
  url.searchParams.set("v", String(v));
  if (token !== undefined) {
    url.searchParams.set("token", token);
  }
  return url.toString();
}

/** A connected, typed hub socket. */
export interface FluenceSocket {
  /** Closes the connection. */
  close(): void;
  /** The underlying socket (escape hatch for advanced uses). */
  readonly raw: WebSocket;
}

/**
 * Opens the hub WebSocket and dispatches incoming frames to the typed
 * handlers. Frames that fail to parse or carry an unhandled topic go to
 * `handlers.unknown` (clients must tolerate newer hubs).
 */
export function openSocket(options: SocketOptions): FluenceSocket {
  const Ctor = options.webSocketCtor ?? WebSocket;
  const url = buildWsUrl(options.baseUrl, options.topics, options.v ?? 1, options.token);
  const socket = new Ctor(url);

  socket.addEventListener("message", (message: MessageEvent) => {
    if (typeof message.data !== "string") {
      options.handlers.unknown?.(message.data);
      return;
    }
    let frame: unknown;
    try {
      frame = JSON.parse(message.data);
    } catch {
      options.handlers.unknown?.(message.data);
      return;
    }
    dispatch(frame, options.handlers);
  });

  return {
    close: () => {
      socket.close();
    },
    raw: socket,
  };
}

/** Routes one parsed frame to its topic handler. */
function dispatch(frame: unknown, handlers: TopicHandlers): void {
  if (typeof frame !== "object" || frame === null || !("topic" in frame) || !("msg" in frame)) {
    handlers.unknown?.(frame);
    return;
  }
  // The frame came from JSON.parse: trust the hub's shape (transport-level
  // SDK, no runtime validation in v0) and let the discriminated union
  // narrow `msg` per topic.
  const typed = frame as ServerFrame;
  switch (typed.topic) {
    case "input":
      if (handlers.input) {
        handlers.input(typed.msg);
        return;
      }
      break;
    case "system":
      if (handlers.system) {
        handlers.system(typed.msg);
        return;
      }
      break;
    default:
      break;
  }
  handlers.unknown?.(frame);
}
