// SPDX-License-Identifier: AGPL-3.0-only

/**
 * Gaze mode for the composer (SPEC §4.D): enable the webcam source, run an
 * express calibration (sequential key fixations → the hub fits the mapping), and
 * record a ground-truth dataset (`record-gaze`, 6.4) compatible with the Rust
 * replay (`fluence_input::GazeSession`). All webcam-dependent (not unit-tested);
 * it composes the tested pure parts ([`GazeSource`], the frame builders).
 */

import { GazeSource } from "./gaze-source.js";

/** A calibration/record target: an id and how to highlight it on screen. */
export interface GazeTarget {
  id: string;
  highlight: (on: boolean) => void;
}

/** What `record-gaze` needs to shape a dataset matching the Rust `GazeSession`. */
export interface SurfaceSnapshot {
  viewport: { w: number; h: number };
  targets: { id: string; rect: number[]; role: string; label: string | null }[];
}

/** Options for a [`GazeController`]. */
export interface GazeControllerOptions {
  socket: WebSocket;
  surface: string;
  /** Spread of targets to fixate during calibration / recording. */
  targets: GazeTarget[];
  /** Current surface snapshot (for the recorded dataset). */
  snapshot: () => SurfaceSnapshot;
  /** Optional progress callback (`done`/`total`) for a UI gauge. */
  onProgress?: (done: number, total: number) => void;
}

/** Samples collected per fixated target during calibration / recording. */
const SAMPLES_PER_TARGET = 6;
/** Dwell on each calibration target (ms) before sampling — let the eye settle. */
const SETTLE_MS = 600;
/** Spacing between samples on a target (ms). */
const SAMPLE_SPACING_MS = 120;

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

/** Drives the webcam gaze mode for one composer session. */
export class GazeController {
  readonly #options: GazeControllerOptions;
  #source: GazeSource | null = null;

  constructor(options: GazeControllerOptions) {
    this.#options = options;
  }

  /** Whether the webcam source is currently running. */
  get enabled(): boolean {
    return this.#source !== null;
  }

  /**
   * Starts the webcam gaze source. Throws if the camera/model is unavailable —
   * the caller keeps the mouse/dwell path (input never depends on the camera).
   */
  async enable(): Promise<void> {
    if (this.#source !== null) {
      return;
    }
    const source = new GazeSource({ socket: this.#options.socket, surface: this.#options.surface });
    await source.start();
    this.#source = source;
  }

  /** Stops the webcam gaze source. Idempotent. */
  disable(): void {
    this.#source?.stop();
    this.#source = null;
  }

  /**
   * Express calibration (SPEC §4.D): fixate a spread of targets, collect a few
   * raw-gaze → target pairs at each, then ask the hub to fit. Returns once the
   * fit request is sent (the hub maps; live quality arrives with the caregiver
   * channel).
   */
  async calibrate(): Promise<void> {
    const source = this.#source;
    if (source === null) {
      throw new Error("enable gaze before calibrating");
    }
    const total = this.#options.targets.length;
    for (const [index, target] of this.#options.targets.entries()) {
      target.highlight(true);
      await sleep(SETTLE_MS);
      for (let i = 0; i < SAMPLES_PER_TARGET; i += 1) {
        source.sendCalibrationSample(target.id);
        await sleep(SAMPLE_SPACING_MS);
      }
      target.highlight(false);
      this.#options.onProgress?.(index + 1, total);
    }
    source.fitCalibration();
  }

  /**
   * `record-gaze` (6.4): captures labelled raw-gaze samples by fixating the same
   * spread, and returns a `GazeSession` (calibration + held-out test split)
   * matching the Rust replay format — real ground truth to evaluate precision.
   */
  async record(name: string): Promise<unknown> {
    const source = this.#source;
    if (source === null) {
      throw new Error("enable gaze before recording");
    }
    const calibration: { features: [number, number]; target: string }[] = [];
    const test: { features: [number, number]; target: string }[] = [];
    const total = this.#options.targets.length;
    for (const [index, target] of this.#options.targets.entries()) {
      target.highlight(true);
      await sleep(SETTLE_MS);
      for (let i = 0; i < SAMPLES_PER_TARGET; i += 1) {
        const raw = source.latest();
        if (raw !== null) {
          const sample = { features: [raw.x, raw.y] as [number, number], target: target.id };
          // First two-thirds calibrate, the rest is held-out test.
          (i < SAMPLES_PER_TARGET - 2 ? calibration : test).push(sample);
        }
        await sleep(SAMPLE_SPACING_MS);
      }
      target.highlight(false);
      this.#options.onProgress?.(index + 1, total);
    }
    const surface = this.#options.snapshot();
    return {
      version: 1,
      name,
      synthetic: false,
      viewport: surface.viewport,
      targets: surface.targets,
      calibration,
      test,
    };
  }
}
