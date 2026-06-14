// SPDX-License-Identifier: AGPL-3.0-only

/**
 * T5 persona scenario (PLAN §1): « urgence : double confirmation obligatoire,
 * bannière reçue par un 2e client appairé ». A single tap only arms; cancelling
 * disarms; only a second tap broadcasts — and a second paired client receives
 * the banner over its `system` topic (D-7.4, SPEC §7.A).
 */

import { openComposer } from "../composer-page.js";
import { expect, test } from "../fixtures.js";

test("emergency requires a double confirm and reaches a second paired client", async ({
  page,
  hub,
  browser,
}) => {
  const tokenA = await hub.pairToken("control");
  const tokenB = await hub.pairToken("control");

  // Client A composes; client B is a second paired device (own context + token).
  await openComposer(page, tokenA);
  const contextB = await browser.newContext({ baseURL: hub.origin });
  try {
    const pageB = await contextB.newPage();
    await openComposer(pageB, tokenB);

    const emergencyA = page.locator(".emergency");
    const cancelA = page.locator(".emergency-cancel");
    const bannerB = pageB.locator(".banner");

    // A single tap only arms — nothing is broadcast, B stays silent.
    await emergencyA.click();
    await expect(emergencyA).toHaveClass(/armed/);
    await expect(cancelA).toBeVisible();
    await expect(bannerB).toBeHidden();

    // Cancelling disarms — still no broadcast.
    await cancelA.click();
    await expect(emergencyA).not.toHaveClass(/armed/);
    await expect(bannerB).toBeHidden();

    // Arm again, then confirm → the alert broadcasts → B receives the banner.
    await emergencyA.click();
    await expect(emergencyA).toHaveClass(/armed/);
    await emergencyA.click();
    await expect(bannerB).toBeVisible();
    await expect(bannerB).toHaveClass(/active/);
  } finally {
    await contextB.close();
  }
});
