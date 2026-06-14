// SPDX-License-Identifier: AGPL-3.0-only

/**
 * T5 persona scenario (PLAN §1): « suggestion acceptée insérée au curseur ».
 * With no LLM configured the hub degrades to the always-on n-gram fallback
 * (D-2.6), which completes the draft's last word — so a real suggestion appears
 * and, once accepted, becomes the draft.
 */

import { clickType, draft, openComposer } from "../composer-page.js";
import { expect, test } from "../fixtures.js";

test("an accepted suggestion becomes the draft", async ({ page, hub }) => {
  const token = await hub.pairToken("control");
  await openComposer(page, token);

  // "bon" → the French base completes it (bon, bonne, bonjour, bonsoir…).
  await clickType(page, "bon");

  const firstSlot = page.locator(".suggestion").first();
  // The composer mirrors a slot's text into its data-text attribute; wait for a
  // non-empty completion to arrive (debounced ~400 ms + anti-flicker gate).
  await expect(firstSlot).toHaveAttribute("data-text", /\S/);
  const suggestion = await firstSlot.getAttribute("data-text");
  expect(suggestion).toBeTruthy();

  await firstSlot.click();
  await expect(draft(page)).toHaveText(suggestion ?? "");
});
