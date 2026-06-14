// SPDX-License-Identifier: AGPL-3.0-only

/**
 * Playwright fixtures for the T5 suite: a per-test assembled hub (`hub`) and a
 * `baseURL` pointing at it, so `page.goto("/")` loads the hub-served composer.
 */

import { test as base } from "@playwright/test";

import { type HubHandle, startHub } from "./hub-harness.js";

interface Fixtures {
  hub: HubHandle;
}

export const test = base.extend<Fixtures>({
  hub: async ({}, use) => {
    const hub = await startHub();
    try {
      await use(hub);
    } finally {
      await hub.stop();
    }
  },
  baseURL: async ({ hub }, use) => {
    await use(hub.origin);
  },
});

export { expect } from "@playwright/test";
