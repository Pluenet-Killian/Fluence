// SPDX-License-Identifier: AGPL-3.0-only

/**
 * Anti-flicker gate for the suggestion slots (SPEC §7.A): at most one update
 * per `minIntervalMs`, and **never while a dwell is past `maxDwellProgress`**
 * (so the targets a user is about to select never move under their gaze).
 */
export class SuggestionGate {
  readonly #minIntervalMs: number;
  readonly #maxDwellProgress: number;
  #lastUpdateMs = Number.NEGATIVE_INFINITY;
  #dwellProgress = 0;

  constructor(minIntervalMs = 600, maxDwellProgress = 0.4) {
    this.#minIntervalMs = minIntervalMs;
    this.#maxDwellProgress = maxDwellProgress;
  }

  /** Records the current dwell progress in `[0, 1]` (0 when not dwelling). */
  setDwellProgress(progress: number): void {
    this.#dwellProgress = progress;
  }

  /** Whether a suggestion update is allowed at `nowMs`. */
  allow(nowMs: number): boolean {
    if (this.#dwellProgress > this.#maxDwellProgress) {
      return false;
    }
    return nowMs - this.#lastUpdateMs >= this.#minIntervalMs;
  }

  /** Records that an update was applied at `nowMs`. */
  mark(nowMs: number): void {
    this.#lastUpdateMs = nowMs;
  }
}
