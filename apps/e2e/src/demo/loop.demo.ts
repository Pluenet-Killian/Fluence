// SPDX-License-Identifier: AGPL-3.0-only

/**
 * The Phase 5 loop, end to end, as a watchable + recorded demo (PLAN §2 Phase 5
 * Done-quand). One person (client A) composes at dwell, accepts a suggestion,
 * speaks (Piper FR when provisioned, else the OS voice) and raises an emergency;
 * a second paired client (B) receives the alert banner. Pauses are deliberate so
 * the recorded video is legible — this is a demo, not the timing-tight T5 suite.
 */

import { draft, dwellType, openComposer } from "../composer-page.js";
import { expect, test } from "../fixtures.js";

/** A readable pause for the camera (not a synchronization primitive). */
const BEAT_MS = 900;

test("the Phase 5 loop: dwell → suggestion → voice → emergency", async ({ page, hub, browser }) => {
  await openComposer(page, await hub.pairToken("control"));

  // A second paired client watches for the emergency banner.
  const contextB = await browser.newContext({ baseURL: hub.origin });
  const pageB = await contextB.newPage();
  await openComposer(pageB, await hub.pairToken("control"));

  try {
    // 1) Compose at dwell (the accessibility path).
    await dwellType(page, "bon");
    await page.waitForTimeout(BEAT_MS);

    // 2) Accept a suggestion — acceleration (the n-gram fallback completes « bon »).
    const suggestion = page.locator(".suggestion").first();
    await expect(suggestion).toHaveAttribute("data-text", /\S/);
    await suggestion.click();
    await expect(draft(page)).not.toHaveText("bon");
    await page.waitForTimeout(BEAT_MS);

    // 3) PARLER — Piper FR when provisioned, else the OS voice ("une voix, toujours").
    await page.locator(".speak").click();
    await page.waitForTimeout(BEAT_MS * 2);

    // 4) Emergency: arm, then *cancel* (the double-confirm protects against misfires)…
    await page.locator(".emergency").click();
    await expect(page.locator(".emergency")).toHaveClass(/armed/);
    await page.waitForTimeout(BEAT_MS);
    await page.locator(".emergency-cancel").click();
    await page.waitForTimeout(BEAT_MS);

    // …then arm again and confirm → the banner reaches the second client.
    await page.locator(".emergency").click();
    await page.locator(".emergency").click();
    await expect(pageB.locator(".banner")).toHaveClass(/active/);
    await page.waitForTimeout(BEAT_MS * 2);
  } finally {
    await contextB.close();
  }
});
