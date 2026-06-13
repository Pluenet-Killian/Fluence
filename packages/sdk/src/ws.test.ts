// SPDX-License-Identifier: Apache-2.0

import { describe, expect, it } from "vitest";

import type { SelectionEvent, SystemEvent } from "./types.js";
import { buildWsUrl, openSocket } from "./ws.js";

describe("buildWsUrl", () => {
  it("negotiates topics and version at open (SPEC §4.A)", () => {
    const url = buildWsUrl("http://127.0.0.1:7411", ["input", "system"], 1, "tok");
    expect(url).toBe("ws://127.0.0.1:7411/ws?topics=input%2Csystem&v=1&token=tok");
  });

  it("upgrades https to wss and omits the token when absent", () => {
    const url = buildWsUrl("https://fluence.local:7411", ["system"], 1);
    expect(url).toBe("wss://fluence.local:7411/ws?topics=system&v=1");
  });
});

/** Minimal WebSocket fake: records the URL, lets tests inject messages. */
class FakeWebSocket {
  static last: FakeWebSocket | undefined;
  readonly url: string;
  closed = false;
  #listeners: ((event: MessageEvent) => void)[] = [];

  constructor(url: string) {
    this.url = url;
    FakeWebSocket.last = this;
  }

  addEventListener(_type: "message", listener: (event: MessageEvent) => void): void {
    this.#listeners.push(listener);
  }

  close(): void {
    this.closed = true;
  }

  emit(data: unknown): void {
    for (const listener of this.#listeners) {
      listener({ data } as MessageEvent);
    }
  }
}

describe("openSocket dispatch", () => {
  function open(handlers: Parameters<typeof openSocket>[0]["handlers"]) {
    return openSocket({
      baseUrl: "http://127.0.0.1:7411",
      topics: ["input", "system"],
      handlers,
      webSocketCtor: FakeWebSocket as unknown as new (url: string) => WebSocket,
    });
  }

  it("routes frames to their topic handler", () => {
    const inputs: SelectionEvent[] = [];
    const systems: SystemEvent[] = [];
    open({ input: (e) => inputs.push(e), system: (e) => systems.push(e) });

    FakeWebSocket.last?.emit(JSON.stringify({ topic: "input", msg: { k: "sel.cancel" } }));
    FakeWebSocket.last?.emit(
      JSON.stringify({ topic: "system", msg: { k: "system.listening", enabled: true } }),
    );

    expect(inputs).toEqual([{ k: "sel.cancel" }]);
    expect(systems).toEqual([{ k: "system.listening", enabled: true }]);
  });

  it("sends unknown topics and malformed frames to the unknown handler", () => {
    const unknown: unknown[] = [];
    open({ unknown: (f) => unknown.push(f) });

    FakeWebSocket.last?.emit(JSON.stringify({ topic: "asr", msg: {} })); // reserved topic
    FakeWebSocket.last?.emit("not json");
    FakeWebSocket.last?.emit(JSON.stringify({ nope: true }));

    expect(unknown).toHaveLength(3);
  });

  it("close() closes the underlying socket", () => {
    const socket = open({});
    socket.close();
    expect(FakeWebSocket.last?.closed).toBe(true);
  });
});
