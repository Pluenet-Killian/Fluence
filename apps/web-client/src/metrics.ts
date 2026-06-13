// SPDX-License-Identifier: AGPL-3.0-only

/**
 * Local usage instrumentation (PLAN 5.5; the ×3 star metric, SPEC §1.2): the
 * user's real effective rate and keystroke savings, computed client-side from
 * their own actions. This is **P2 data** — it stays on this device; encrypted
 * hub storage for cross-session aggregation is deferred (debt).
 *
 * - *effective WPM* = produced characters / 5, over the elapsed minutes.
 * - *keystroke savings* = how much of the produced text the engine spared,
 *   `(produced − selections) / produced`: one selection that drops a whole
 *   suggestion in scores high, char-by-char typing scores zero.
 */
export interface Metrics {
  /** Effective words per minute. */
  wpm: number;
  /** Keystroke savings, in percent. */
  ksPercent: number;
}

const CHARS_PER_WORD = 5;
const MS_PER_MINUTE = 60_000;

/** Accumulates selection actions and turns them into live [`Metrics`]. */
export class UsageMeter {
  #firstActionMs: number | null = null;
  #selections = 0;

  /** Records one selection action (a key, a backspace, an accepted suggestion). */
  recordSelection(nowMs: number): void {
    this.#firstActionMs ??= nowMs;
    this.#selections += 1;
  }

  /** Current metrics given the produced text length and the current time. */
  snapshot(producedChars: number, nowMs: number): Metrics {
    const minutes =
      this.#firstActionMs === null ? 0 : (nowMs - this.#firstActionMs) / MS_PER_MINUTE;
    const words = producedChars / CHARS_PER_WORD;
    const wpm = minutes > 0 ? words / minutes : 0;
    const ksPercent =
      producedChars > 0
        ? Math.max(0, ((producedChars - this.#selections) / producedChars) * 100)
        : 0;
    return { wpm, ksPercent };
  }
}
