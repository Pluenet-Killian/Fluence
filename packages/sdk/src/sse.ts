// SPDX-License-Identifier: Apache-2.0

/**
 * Minimal SSE parser over a `ReadableStream` (SPEC §5.A: `/suggest`
 * streams over SSE).
 *
 * `EventSource` cannot send `POST` bodies or the `X-Fluence-Token` header,
 * so the SDK parses the stream from `fetch` itself. The wire maps onto the
 * canonical event form: the SSE `event:` field is the variant tag, `data:`
 * is the JSON payload — together they rebuild `{ event, data }` exactly as
 * the schema documents.
 *
 * @packageDocumentation
 */

/** One parsed SSE event before typing. */
export interface RawSseEvent {
  /** The `event:` field (defaults to `"message"` per the SSE spec). */
  event: string;
  /** The concatenated `data:` lines. */
  data: string;
}

/**
 * Parses an SSE byte stream into events, handling chunk fragmentation
 * (events split across network chunks reassemble correctly).
 */
export async function* parseSseStream(
  stream: ReadableStream<Uint8Array>,
): AsyncGenerator<RawSseEvent, void, undefined> {
  const decoder = new TextDecoder();
  const reader = stream.getReader();
  let buffer = "";
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) {
        break;
      }
      buffer += decoder.decode(value, { stream: true });
      // Events are separated by a blank line; normalize CRLF first.
      buffer = buffer.replace(/\r\n/g, "\n");
      for (;;) {
        const boundary = buffer.indexOf("\n\n");
        if (boundary === -1) {
          break;
        }
        const block = buffer.slice(0, boundary);
        buffer = buffer.slice(boundary + 2);
        const event = parseBlock(block);
        if (event !== undefined) {
          yield event;
        }
      }
    }
  } finally {
    reader.releaseLock();
  }
}

/** Parses one SSE block (lines until a blank line). */
function parseBlock(block: string): RawSseEvent | undefined {
  let event = "message";
  const data: string[] = [];
  for (const line of block.split("\n")) {
    if (line.startsWith(":")) {
      continue; // comment / keep-alive
    }
    const colon = line.indexOf(":");
    if (colon === -1) {
      continue;
    }
    const field = line.slice(0, colon);
    // Per the SSE spec a single leading space after the colon is stripped.
    const value = line.slice(colon + 1).replace(/^ /, "");
    if (field === "event") {
      event = value;
    } else if (field === "data") {
      data.push(value);
    }
  }
  if (data.length === 0) {
    return undefined;
  }
  return { event, data: data.join("\n") };
}
