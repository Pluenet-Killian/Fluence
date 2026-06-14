// SPDX-License-Identifier: AGPL-3.0-only

/**
 * T5 persona scenario (PLAN §1): « le hub tué pendant la frappe → l'UI se
 * reconnecte, draft intact ». The hub crashes mid-session; the composer shows
 * "reconnecting", then recovers on the same port and keeps the typed draft.
 */

import { clickType, draft, openComposer } from "../composer-page.js";
import { expect, test } from "../fixtures.js";

test("a hub killed mid-typing: the UI reconnects and the draft survives", async ({ page, hub }) => {
  const token = await hub.pairToken("control");
  await openComposer(page, token);

  await clickType(page, "salut");
  await expect(draft(page)).toHaveText("salut");

  const status = page.locator(".status");

  // Crash the hub: the socket drops, the composer announces it is reconnecting.
  await hub.kill();
  await expect(status).toHaveAttribute("data-state", "status.reconnecting");

  // Recover on the same port: the composer's retry succeeds and it reconnects…
  await hub.start();
  await expect(status).toHaveAttribute("data-state", "status.connected");

  // …and the draft typed before the crash is still there (intact in the UI).
  await expect(draft(page)).toHaveText("salut");
});
