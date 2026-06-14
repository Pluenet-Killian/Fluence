// SPDX-License-Identifier: Apache-2.0

import { describe, expect, expectTypeOf, it } from "vitest";

import { FluenceClient, FluenceProblemError } from "./client.js";
import type {
  DeviceList,
  HealthResponse,
  NextCharsResponse,
  PairResponse,
  Problem,
  SuggestEvent,
} from "./types.js";

/** Records calls and replays canned responses, in order. */
function mockFetch(responses: Response[]): {
  fetch: typeof globalThis.fetch;
  calls: { url: string; init: RequestInit }[];
} {
  const calls: { url: string; init: RequestInit }[] = [];
  const queue = [...responses];
  const fetch = ((input: string | URL | Request, init?: RequestInit) => {
    const url =
      typeof input === "string" ? input : input instanceof URL ? input.toString() : input.url;
    calls.push({ url, init: init ?? {} });
    const next = queue.shift();
    if (next === undefined) {
      throw new Error("mockFetch: no response queued");
    }
    return Promise.resolve(next);
  }) as typeof globalThis.fetch;
  return { fetch, calls };
}

const BASE = "http://127.0.0.1:7411";

describe("FluenceClient transport", () => {
  it("sends the device token and JSON body to the right URL", async () => {
    const { fetch, calls } = mockFetch([Response.json({ session_id: "s1" })]);
    const client = new FluenceClient({ baseUrl: BASE, token: "tok_1", fetch });

    const created = await client.createSession();
    expect(created.session_id).toBe("s1");
    expect(calls[0]?.url).toBe(`${BASE}/api/v1/sessions`);
    const headers = calls[0]?.init.headers as Record<string, string>;
    expect(headers["X-Fluence-Token"]).toBe("tok_1");
  });

  it("escapes path parameters", async () => {
    const { fetch, calls } = mockFetch([new Response(null, { status: 204 })]);
    const client = new FluenceClient({ baseUrl: BASE, token: "t", fetch });

    await client.deleteSession("a/b c");
    expect(calls[0]?.url).toBe(`${BASE}/api/v1/sessions/a%2Fb%20c`);
  });

  it("posts the emergency state to the system endpoint", async () => {
    const { fetch, calls } = mockFetch([new Response(null, { status: 204 })]);
    const client = new FluenceClient({ baseUrl: BASE, token: "t", fetch });

    await client.emergency(true);
    expect(calls[0]?.url).toBe(`${BASE}/api/v1/system/emergency`);
    expect(calls[0]?.init.method).toBe("POST");
    expect(JSON.parse(calls[0]?.init.body as string)).toEqual({ active: true });
  });

  it("surfaces problem+json errors as FluenceProblemError", async () => {
    const problem: Problem = {
      type: "urn:fluence:problem:scope_insufficient",
      title: "Insufficient scope",
      status: 403,
      code: "scope_insufficient",
    };
    const { fetch } = mockFetch([Response.json(problem, { status: 403 })]);
    const client = new FluenceClient({ baseUrl: BASE, token: "t", fetch });

    const error = await client.health().catch((e: unknown) => e);
    expect(error).toBeInstanceOf(FluenceProblemError);
    expect((error as FluenceProblemError).problem.code).toBe("scope_insufficient");
  });

  it("synthesizes an unknown-coded problem for non-JSON failures", async () => {
    const { fetch } = mockFetch([
      new Response("Bad Gateway", { status: 502, statusText: "Bad Gateway" }),
    ]);
    const client = new FluenceClient({ baseUrl: BASE, token: "t", fetch });

    const error = await client.health().catch((e: unknown) => e);
    expect((error as FluenceProblemError).problem.code).toBe("unknown");
    expect((error as FluenceProblemError).problem.status).toBe(502);
  });

  it("streams typed suggest events from SSE", async () => {
    const sse = [
      'event: delta\ndata: {"i":0,"text":"Je voudrais de l\'eau"}\n\n',
      'event: final\ndata: {"suggestions":[{"text":"Je voudrais de l\'eau fraîche.","score":0.9}]}\n\n',
    ].join("");
    const { fetch, calls } = mockFetch([
      new Response(sse, { headers: { "Content-Type": "text/event-stream" } }),
    ]);
    const client = new FluenceClient({ baseUrl: BASE, token: "t", fetch });

    const events: SuggestEvent[] = [];
    for await (const event of client.suggest("s1", {
      mode: "rephrase",
      draft: "veu eau frache",
      n: 3,
      slot: "main",
    })) {
      events.push(event);
    }

    expect(events).toHaveLength(2);
    expect(events[0]).toEqual({
      event: "delta",
      data: { i: 0, text: "Je voudrais de l'eau" },
    });
    const final = events[1];
    if (final?.event !== "final") {
      throw new Error("expected final event");
    }
    expect(final.data.suggestions[0]?.score).toBe(0.9);
    expect(calls[0]?.url).toBe(`${BASE}/api/v1/sessions/s1/suggest`);
  });

  it("encodes query parameters (next-chars prefix)", async () => {
    const { fetch, calls } = mockFetch([Response.json({ dist: [] })]);
    const client = new FluenceClient({ baseUrl: BASE, token: "t", fetch });

    await client.nextChars("s1", "bonjou r&é");
    expect(calls[0]?.url).toBe(`${BASE}/api/v1/sessions/s1/next-chars?prefix=bonjou%20r%26%C3%A9`);
  });

  it("lists devices and the access journal for the caregiver space", async () => {
    const { fetch, calls } = mockFetch([
      Response.json({ devices: [] }),
      Response.json({ entries: [] }),
    ]);
    const client = new FluenceClient({ baseUrl: BASE, token: "care", fetch });

    await client.devices();
    expect(calls[0]?.url).toBe(`${BASE}/api/v1/devices`);

    await client.journal(20);
    expect(calls[1]?.url).toBe(`${BASE}/api/v1/system/journal?limit=20`);
  });

  it("revokes a device by id (DELETE, path-escaped)", async () => {
    const { fetch, calls } = mockFetch([new Response(null, { status: 204 })]);
    const client = new FluenceClient({ baseUrl: BASE, token: "care", fetch });

    await client.revokeDevice("dev/2 x");
    expect(calls[0]?.init.method).toBe("DELETE");
    expect(calls[0]?.url).toBe(`${BASE}/api/v1/devices/dev%2F2%20x`);
  });
});

describe("FluenceClient types (T3: the generated SDK compiles and is typed)", () => {
  it("exposes typed responses", () => {
    expectTypeOf<ReturnType<FluenceClient["health"]>>().resolves.toEqualTypeOf<HealthResponse>();
    expectTypeOf<
      ReturnType<FluenceClient["nextChars"]>
    >().resolves.toEqualTypeOf<NextCharsResponse>();
    expectTypeOf<ReturnType<FluenceClient["pair"]>>().resolves.toEqualTypeOf<PairResponse>();
    expectTypeOf<ReturnType<FluenceClient["devices"]>>().resolves.toEqualTypeOf<DeviceList>();
    // The SSE generator yields the discriminated union, narrowable on
    // `event` — exactly the canonical form of the schema.
    expectTypeOf<SuggestEvent>().toExtend<{ event: "delta" | "final" | "aborted" }>();
  });
});
