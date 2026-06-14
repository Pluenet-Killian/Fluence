// SPDX-License-Identifier: AGPL-3.0-only

import { defineConfig, devices } from "@playwright/test";

/**
 * The reproducible "filmed" demo of the Phase 5 loop (PLAN §2 Phase 5
 * Done-quand): compose at dwell, accept a suggestion, PARLER (Piper FR when
 * `FLUENCE_PIPER_BIN`/`FLUENCE_PIPER_VOICE` are exported — the harness forwards
 * them — else the OS voice), then trigger and cancel an emergency.
 *
 * Runs headed and records a video to `demo-output/`. See `docs/demos/phase5-loop.md`.
 */
export default defineConfig({
  testDir: "./src/demo",
  testMatch: /.*\.demo\.ts/,
  fullyParallel: false,
  workers: 1,
  timeout: 120_000,
  outputDir: "./demo-output",
  use: {
    ...devices["Desktop Chrome"],
    headless: false,
    viewport: { width: 1280, height: 720 },
    video: { mode: "on", size: { width: 1280, height: 720 } },
  },
});
