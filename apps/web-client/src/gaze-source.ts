// SPDX-License-Identifier: AGPL-3.0-only

/**
 * Webcam gaze source (SPEC §4.A level 2, §4.C step 1): runs MediaPipe Face
 * Landmarker on the camera, turns each frame into a raw gaze point
 * ([`estimateGaze`]) and streams it to the hub as a `gaze:` pointer sample —
 * the hub calibrates and fuses (D-4.1). Also drives calibration (`cal.sample` /
 * `cal.fit`, SPEC §4.D).
 *
 * **Offline (SPEC §1)**: the MediaPipe WASM and the face-landmarker model are
 * loaded from **local** paths (provisioned like Piper — see the demo doc), never
 * a CDN. MediaPipe is **dynamically imported** so the mouse composer never pays
 * for it (and the e2e suite, which is mouse-only, is untouched).
 *
 * The webcam/MediaPipe glue cannot be unit-tested headlessly; the pure parts it
 * builds on ([`estimateGaze`], the frame builders) are tested separately.
 */

import type { FaceLandmarker } from "@mediapipe/tasks-vision";

import { estimateGaze, extractGazeLandmarks, type RawGaze } from "./gaze-estimate.js";
import { calibrationFitFrame, calibrationSampleFrame, gazePointerFrame } from "./gaze-frames.js";

/** Source id of the webcam gaze (SPEC §4.A `kind:instance` convention). */
const GAZE_SOURCE = "gaze:webcam";
/** Pointer sampling throttle (ms) — matches the composer's pointer cadence. */
const SAMPLE_THROTTLE_MS = 50;
/** Default local asset locations (provisioned offline). */
const DEFAULT_WASM_PATH = "/mediapipe/wasm";
const DEFAULT_MODEL_PATH = "/models/face_landmarker.task";

/** Options for a [`GazeSource`]. */
export interface GazeSourceOptions {
  /** The composer's raw socket, used to send `input` frames. */
  socket: WebSocket;
  /** Surface being driven (must match the declared target map). */
  surface: string;
  /** Local MediaPipe WASM directory (offline). */
  wasmPath?: string;
  /** Local face-landmarker model path (offline). */
  modelPath?: string;
  /** Observer of each raw gaze estimate (for an on-screen quality indicator). */
  onRaw?: (raw: RawGaze) => void;
}

/**
 * A running webcam gaze source. Construct, `await start()`, and it streams gaze
 * to the hub until `stop()`. Calibration is driven by the owner (e.g. a
 * smooth-pursuit / express sequence) via `sendCalibrationSample` + `fit`.
 */
export class GazeSource {
  readonly #options: GazeSourceOptions;
  readonly #video: HTMLVideoElement;
  #landmarker: FaceLandmarker | null = null;
  #stream: MediaStream | null = null;
  #running = false;
  #lastSampleMs = 0;
  #latest: RawGaze | null = null;
  #lastVideoTime = -1;

  constructor(options: GazeSourceOptions) {
    this.#options = options;
    this.#video = document.createElement("video");
    this.#video.playsInline = true;
    this.#video.muted = true;
  }

  /** The most recent raw gaze estimate, if any (used by calibration capture). */
  latest(): RawGaze | null {
    return this.#latest;
  }

  /**
   * Opens the camera, loads the local MediaPipe model, and starts streaming.
   *
   * @throws if the camera is denied or the model/WASM cannot be loaded (the
   * caller falls back to the click/dwell-mouse path — input never depends on the
   * camera being up).
   */
  async start(): Promise<void> {
    this.#stream = await navigator.mediaDevices.getUserMedia({
      video: { facingMode: "user", width: 640, height: 480 },
    });
    this.#video.srcObject = this.#stream;
    await this.#video.play();

    // Dynamic import: MediaPipe only loads when gaze is actually used.
    const { FaceLandmarker, FilesetResolver } = await import("@mediapipe/tasks-vision");
    const fileset = await FilesetResolver.forVisionTasks(
      this.#options.wasmPath ?? DEFAULT_WASM_PATH,
    );
    this.#landmarker = await FaceLandmarker.createFromOptions(fileset, {
      baseOptions: { modelAssetPath: this.#options.modelPath ?? DEFAULT_MODEL_PATH },
      runningMode: "VIDEO",
      numFaces: 1,
    });

    this.#running = true;
    this.#loop();
  }

  /** Stops streaming and releases the camera + model. Idempotent. */
  stop(): void {
    this.#running = false;
    this.#stream?.getTracks().forEach((track) => {
      track.stop();
    });
    this.#stream = null;
    this.#landmarker?.close();
    this.#landmarker = null;
  }

  /** Sends a calibration pair: the latest raw gaze, labelled with `target`. */
  sendCalibrationSample(target: string): void {
    const raw = this.#latest;
    if (raw === null || this.#options.socket.readyState !== WebSocket.OPEN) {
      return;
    }
    this.#options.socket.send(
      JSON.stringify(calibrationSampleFrame(this.#options.surface, target, raw.x, raw.y)),
    );
  }

  /** Asks the hub to fit the calibration from the collected pairs. */
  fitCalibration(): void {
    if (this.#options.socket.readyState === WebSocket.OPEN) {
      this.#options.socket.send(JSON.stringify(calibrationFitFrame(this.#options.surface)));
    }
  }

  #loop(): void {
    if (!this.#running || this.#landmarker === null) {
      return;
    }
    // Only run detection on a fresh frame (detectForVideo needs increasing time).
    if (this.#video.currentTime !== this.#lastVideoTime) {
      this.#lastVideoTime = this.#video.currentTime;
      const result = this.#landmarker.detectForVideo(this.#video, performance.now());
      const faces = result.faceLandmarks;
      if (faces.length > 0 && faces[0] !== undefined) {
        const landmarks = extractGazeLandmarks(faces[0]);
        if (landmarks !== null) {
          const raw = estimateGaze(landmarks);
          this.#latest = raw;
          this.#options.onRaw?.(raw);
          this.#streamSample(raw);
        }
      }
    }
    requestAnimationFrame(() => {
      this.#loop();
    });
  }

  #streamSample(raw: RawGaze): void {
    const now = performance.now();
    if (now - this.#lastSampleMs < SAMPLE_THROTTLE_MS) {
      return;
    }
    this.#lastSampleMs = now;
    if (this.#options.socket.readyState !== WebSocket.OPEN) {
      return;
    }
    this.#options.socket.send(
      JSON.stringify(gazePointerFrame(GAZE_SOURCE, raw.x, raw.y, raw.conf, Math.round(now * 1000))),
    );
  }
}
