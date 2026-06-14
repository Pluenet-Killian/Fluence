// SPDX-License-Identifier: AGPL-3.0-only

/**
 * Pure webcam-gaze estimation (SPEC §4.C step 1): turn face-mesh landmarks into
 * a **raw** normalized gaze point. It only has to be *monotonic* with gaze
 * direction — the hub's ridge calibration learns the exact mapping to the screen
 * (the simple, debuggable v0; SPEC §4.C). Decoupled from MediaPipe (it takes
 * plain points) so the geometry is unit-tested without a model or a camera.
 */

/** A 2-D landmark in normalized image coordinates `[0, 1]`. */
export interface Point {
  x: number;
  y: number;
}

/** The five landmarks of one eye used for the estimate. */
export interface Eye {
  /** Iris centre. */
  iris: Point;
  /** Inner/outer corners and upper/lower lids (orientation-agnostic). */
  left: Point;
  right: Point;
  top: Point;
  bottom: Point;
}

/** Both eyes' landmarks. */
export interface GazeLandmarks {
  left: Eye;
  right: Eye;
}

/** A raw gaze estimate: normalized point + confidence, all in `[0, 1]`. */
export interface RawGaze {
  x: number;
  y: number;
  conf: number;
}

/**
 * MediaPipe Face Landmarker indices (with `refineLandmarks: true`, 478 points).
 * Iris centres are the refined points; the eye box uses standard mesh corners
 * and lids. (Exact indices drive *real* accuracy; the calibration corrects the
 * rest — that is the v0 contract, SPEC §4.C.)
 */
export const LANDMARK_INDEX = {
  leftIris: 468,
  rightIris: 473,
  leftCornerA: 33,
  leftCornerB: 133,
  leftLidTop: 159,
  leftLidBottom: 145,
  rightCornerA: 362,
  rightCornerB: 263,
  rightLidTop: 386,
  rightLidBottom: 374,
} as const;

/** Minimum landmark count for the refined (iris) mesh. */
const REFINED_LANDMARK_COUNT = 478;

/** Below this eye-openness (normalized lid gap) the eye is treated as shut. */
const MIN_EYE_OPENNESS = 1e-3;

function ratio(value: number, a: number, b: number): number {
  const lo = Math.min(a, b);
  const hi = Math.max(a, b);
  if (hi - lo < MIN_EYE_OPENNESS) {
    return 0.5; // degenerate: assume centred
  }
  return Math.min(1, Math.max(0, (value - lo) / (hi - lo)));
}

/**
 * Picks the gaze landmarks from a Face Landmarker result. Returns `null` when
 * the mesh is not the refined (iris) one — gaze needs the iris points.
 */
export function extractGazeLandmarks(landmarks: readonly Point[]): GazeLandmarks | null {
  if (landmarks.length < REFINED_LANDMARK_COUNT) {
    return null;
  }
  const at = (index: number): Point => {
    const point = landmarks[index];
    // Length is checked above; this keeps the function total for the type system.
    return point ?? { x: 0.5, y: 0.5 };
  };
  return {
    left: {
      iris: at(LANDMARK_INDEX.leftIris),
      left: at(LANDMARK_INDEX.leftCornerA),
      right: at(LANDMARK_INDEX.leftCornerB),
      top: at(LANDMARK_INDEX.leftLidTop),
      bottom: at(LANDMARK_INDEX.leftLidBottom),
    },
    right: {
      iris: at(LANDMARK_INDEX.rightIris),
      left: at(LANDMARK_INDEX.rightCornerA),
      right: at(LANDMARK_INDEX.rightCornerB),
      top: at(LANDMARK_INDEX.rightLidTop),
      bottom: at(LANDMARK_INDEX.rightLidBottom),
    },
  };
}

/** The iris position within one eye's box, as `(x, y)` in `[0, 1]`. */
function eyeGaze(eye: Eye): { x: number; y: number; open: boolean } {
  const open = Math.abs(eye.bottom.y - eye.top.y) >= MIN_EYE_OPENNESS;
  return {
    x: ratio(eye.iris.x, eye.left.x, eye.right.x),
    y: ratio(eye.iris.y, eye.top.y, eye.bottom.y),
    open,
  };
}

/**
 * Estimates the raw gaze point from both eyes: the iris position within each eye
 * box, averaged. Closed eyes drop to low confidence (the I-VT/loss handling on
 * the hub then pauses the dwell). Monotonic with gaze direction by construction.
 */
export function estimateGaze(landmarks: GazeLandmarks): RawGaze {
  const left = eyeGaze(landmarks.left);
  const right = eyeGaze(landmarks.right);
  const openCount = Number(left.open) + Number(right.open);
  return {
    x: (left.x + right.x) / 2,
    y: (left.y + right.y) / 2,
    conf: openCount / 2,
  };
}
