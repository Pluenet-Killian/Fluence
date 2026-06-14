// SPDX-License-Identifier: AGPL-3.0-only

import { defineConfig, devices } from "@playwright/test";

const isCi = process.env["CI"] !== undefined;

/**
 * T5 end-to-end persona suite (PLAN §1, §2 Phase 5). Each test spawns its own
 * assembled hub (real binary, n-gram fallback, OS voice) serving the built web
 * composer — see `src/hub-harness.ts`. Serial (`workers: 1`): a test owns a real
 * hub process and one test kills it mid-run, so isolation beats parallelism.
 */
export default defineConfig({
  testDir: "./src/specs",
  fullyParallel: false,
  workers: 1,
  forbidOnly: isCi,
  retries: isCi ? 1 : 0,
  reporter: isCi ? [["github"], ["list"]] : [["list"]],
  timeout: 90_000,
  expect: { timeout: 15_000 },
  use: {
    ...devices["Desktop Chrome"],
    trace: "retain-on-failure",
    video: "retain-on-failure",
  },
});
