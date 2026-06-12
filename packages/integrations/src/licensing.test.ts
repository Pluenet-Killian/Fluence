// SPDX-License-Identifier: Apache-2.0

import { readFileSync } from "node:fs";
import { expect, it } from "vitest";

// D-10.1: packages/ are reusable bricks, licensed Apache-2.0. This test also
// validates the Phase 0 plumbing (vitest + tsc reach this package).
it("package license follows D-10.1", () => {
  const manifestUrl = new URL("../package.json", import.meta.url);
  const manifest = JSON.parse(readFileSync(manifestUrl, "utf8")) as { license?: string };
  expect(manifest.license).toBe("Apache-2.0");
});
