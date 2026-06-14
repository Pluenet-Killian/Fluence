// SPDX-License-Identifier: AGPL-3.0-only

/**
 * T5 persona scenario (PLAN §1): « Marc compose "bonjour" au dwell-souris et le
 * fait parler » — commits land, audio is emitted, the draft is autosaved.
 */

import { draft, dwellType, openComposer } from "../composer-page.js";
import { expect, test } from "../fixtures.js";

test('Marc composes "bonjour" by mouse-dwell, speaks it, and the draft is autosaved', async ({
  page,
  hub,
  request,
}) => {
  const token = await hub.pairToken("control");

  // Capture every draft autosave (PUT /sessions/{id}/draft) the composer makes.
  const autosaved: string[] = [];
  page.on("request", (request) => {
    if (request.method() === "PUT" && request.url().includes("/draft")) {
      const body = request.postDataJSON() as { text?: string } | null;
      if (body?.text !== undefined) {
        autosaved.push(body.text);
      }
    }
  });

  await openComposer(page, token);

  // 1) Compose the whole word purely by dwell: each letter commits hub-side.
  await dwellType(page, "bonjour");
  await expect(draft(page)).toHaveText("bonjour");

  // 2) PARLER → the UI calls the hub, which answers with audio (OS voice in CI,
  // Piper locally). The browser consumes the audio stream (it plays it), so the
  // intercepted response only proves the status/type of the UI path…
  const speakResponse = page.waitForResponse((response) => response.url().includes("/voice/speak"));
  await page.locator(".speak").click();
  const response = await speakResponse;
  expect(response.status()).toBe(200);
  expect(response.headers()["content-type"]).toContain("audio/wav");

  // …so assert real, non-empty audio bytes through an independent request whose
  // body is not consumed by the page: the hub serves a valid, non-silent WAV
  // ("une voix, toujours", SPEC §2.C — never a 200-with-0-bytes).
  const direct = await request.post(`${hub.origin}/api/v1/voice/speak`, {
    headers: { "x-fluence-token": token },
    data: { text: "bonjour", voice_id: "system:default" },
  });
  expect(direct.ok()).toBeTruthy();
  const wav = await direct.body();
  expect(wav.byteLength).toBeGreaterThan(44);
  expect(wav.subarray(0, 4).toString("latin1")).toBe("RIFF");

  // 3) The composed text was autosaved continuously (D-2.6).
  await expect.poll(() => autosaved.at(-1)).toBe("bonjour");
});
