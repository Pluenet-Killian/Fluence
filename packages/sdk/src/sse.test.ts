// SPDX-License-Identifier: Apache-2.0

import { describe, expect, it } from "vitest";

import { parseSseStream, type RawSseEvent } from "./sse.js";

/** Builds a byte stream from chunks (simulating network fragmentation). */
function streamOf(chunks: string[]): ReadableStream<Uint8Array> {
  const encoder = new TextEncoder();
  return new ReadableStream({
    start(controller) {
      for (const chunk of chunks) {
        controller.enqueue(encoder.encode(chunk));
      }
      controller.close();
    },
  });
}

async function collect(stream: ReadableStream<Uint8Array>): Promise<RawSseEvent[]> {
  const events: RawSseEvent[] = [];
  for await (const event of parseSseStream(stream)) {
    events.push(event);
  }
  return events;
}

describe("parseSseStream", () => {
  it("parses a suggest-style stream (delta, final)", async () => {
    const events = await collect(
      streamOf([
        'event: delta\ndata: {"i":0,"text":"Je voudrais"}\n\n',
        'event: final\ndata: {"suggestions":[]}\n\n',
      ]),
    );
    expect(events).toEqual([
      { event: "delta", data: '{"i":0,"text":"Je voudrais"}' },
      { event: "final", data: '{"suggestions":[]}' },
    ]);
  });

  it("reassembles events split across chunks", async () => {
    // One event fragmented mid-line and mid-separator — the realistic
    // network case the parser exists for.
    const events = await collect(
      streamOf(["event: del", 'ta\ndata: {"i":0,"te', 'xt":"a"}\n', "\n"]),
    );
    expect(events).toEqual([{ event: "delta", data: '{"i":0,"text":"a"}' }]);
  });

  it("ignores comments and events without data", async () => {
    const events = await collect(
      streamOf([": keep-alive\n\n", "event: delta\n\n", 'data: {"x":1}\n\n']),
    );
    // The bare `event:` block carries no data → skipped; the bare data
    // block defaults to event "message" per the SSE spec.
    expect(events).toEqual([{ event: "message", data: '{"x":1}' }]);
  });

  it("joins multi-line data and handles CRLF", async () => {
    const events = await collect(streamOf(['event: x\r\ndata: {"a":\r\ndata: 1}\r\n\r\n']));
    expect(events).toEqual([{ event: "x", data: '{"a":\n1}' }]);
  });
});
