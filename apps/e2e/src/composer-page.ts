// SPDX-License-Identifier: AGPL-3.0-only

/**
 * Page-level helpers driving the real web composer (SPEC §7.A) the way a person
 * would: a paired token is injected into `localStorage` (as the connect screen
 * would store it), then typing happens two ways — `dwellType` exercises the
 * accessibility path (mouse-dwell → hub hit-test + dwell timer → commit) and
 * `clickType` the universal direct-click path.
 */

import { expect, type Locator, type Page } from "@playwright/test";

const TOKEN_KEY = "fluence.token";
/** Poll cadence for the dwell jiggle (ms). Below the composer's 50 ms pointer
 * throttle would drop samples; 60 ms keeps a steady on-target stream. */
const JIGGLE_MS = 60;
/** Safety cap per key: comfortably above base dwell (800 ms) + cooldown. */
const DWELL_DEADLINE_MS = 9_000;

/** The on-screen target id for a character (letters → `key_x`, space → `key_space`). */
function keyId(ch: string): string {
  return ch === " " ? "key_space" : `key_${ch}`;
}

/**
 * Boots the composer with `token`: injects it before navigation, loads the
 * hub-served PWA, and waits until the keyboard is up and the socket connected.
 */
export async function openComposer(page: Page, token: string): Promise<void> {
  await page.addInitScript(
    (args: readonly [string, string]) => {
      window.localStorage.setItem(args[0], args[1]);
    },
    [TOKEN_KEY, token] as const,
  );
  await page.goto("/");
  await expect(page.locator(".keyboard")).toBeVisible();
  // The composer sets data-state on the status element once the socket opens.
  await expect(page.locator(".status")).toHaveAttribute("data-state", "status.connected");
}

/** The live draft text (the `<output class="draft">`). */
export function draft(page: Page): Locator {
  return page.locator(".draft");
}

/** Types `word` via direct clicks — deterministic and instant. */
export async function clickType(page: Page, word: string): Promise<void> {
  for (const ch of word) {
    await page.locator(`.key[data-id="${keyId(ch)}"]`).click();
  }
}

/**
 * Types `word` by mouse-dwell: for each key, hover its centre and jiggle ±1px so
 * the composer keeps streaming pointer samples; the hub accumulates wall-clock
 * dwell and commits, which the composer turns into a keypress. Polls until the
 * draft grows, so it is robust to CI timing jitter (slower only means more
 * elapsed dwell, never a miss).
 */
export async function dwellType(page: Page, word: string): Promise<void> {
  const output = draft(page);
  let expected = "";
  for (const ch of word) {
    expected += ch;
    await dwellKey(page, keyId(ch), expected, output);
  }
}

async function dwellKey(
  page: Page,
  id: string,
  expectedDraft: string,
  output: Locator,
): Promise<void> {
  const box = await page.locator(`.key[data-id="${id}"]`).boundingBox();
  if (box === null) {
    throw new Error(`key ${id} is not visible`);
  }
  const cx = box.x + box.width / 2;
  const cy = box.y + box.height / 2;
  const deadline = Date.now() + DWELL_DEADLINE_MS;
  let tick = 0;
  while ((await output.textContent()) !== expectedDraft && Date.now() < deadline) {
    // ±1px stays well inside the key (forces a fresh pointermove each tick).
    await page.mouse.move(cx + (tick % 2), cy);
    tick += 1;
    await page.waitForTimeout(JIGGLE_MS);
  }
  await expect(output).toHaveText(expectedDraft);
}
