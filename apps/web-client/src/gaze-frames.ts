// SPDX-License-Identifier: AGPL-3.0-only

/**
 * Wire frames for the gaze input path (contract `InputClientMessage`). Built
 * explicitly because contract-gen drops the `k` tag for newtype enum variants
 * (the same quirk the pointer frame works around) — kept pure and tested so the
 * exact shapes the hub deserializes can't drift.
 */

/** A `gaze:` pointer sample (raw gaze the hub calibrates), `input` topic. */
export interface GazePointerFrame {
  topic: "input";
  msg: { k: "ptr"; t: number; src: string; x: number; y: number; conf: number };
}

/** A `cal.sample` frame: a raw-gaze → target calibration pair (SPEC §4.D). */
export interface CalibrationSampleFrame {
  topic: "input";
  msg: { k: "cal.sample"; surface: string; target: string; x: number; y: number };
}

/** A `cal.fit` frame: fit the calibration for a surface (SPEC §4.D). */
export interface CalibrationFitFrame {
  topic: "input";
  msg: { k: "cal.fit"; surface: string };
}

/** Clamps to `[0, 1]` (raw gaze is normalized to the surface). */
function unit(value: number): number {
  return Math.min(1, Math.max(0, value));
}

/** Builds a `gaze:` pointer frame at monotonic time `t` (µs). */
export function gazePointerFrame(
  source: string,
  x: number,
  y: number,
  conf: number,
  t: number,
): GazePointerFrame {
  return {
    topic: "input",
    msg: { k: "ptr", t, src: source, x: unit(x), y: unit(y), conf: unit(conf) },
  };
}

/** Builds a `cal.sample` frame (raw gaze `(x, y)` while looking at `target`). */
export function calibrationSampleFrame(
  surface: string,
  target: string,
  x: number,
  y: number,
): CalibrationSampleFrame {
  return {
    topic: "input",
    msg: { k: "cal.sample", surface, target, x: unit(x), y: unit(y) },
  };
}

/** Builds a `cal.fit` frame for `surface`. */
export function calibrationFitFrame(surface: string): CalibrationFitFrame {
  return { topic: "input", msg: { k: "cal.fit", surface } };
}
