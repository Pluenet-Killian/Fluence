// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from "vitest";

import {
  estimateGaze,
  extractGazeLandmarks,
  LANDMARK_INDEX,
  type Eye,
  type Point,
} from "./gaze-estimate.js";

/** An eye box centred on (cx, cy) of half-size `r`, with the iris at `iris`. */
function eye(cx: number, cy: number, r: number, iris: Point): Eye {
  return {
    iris,
    left: { x: cx - r, y: cy },
    right: { x: cx + r, y: cy },
    top: { x: cx, y: cy - r },
    bottom: { x: cx, y: cy + r },
  };
}

describe("estimateGaze", () => {
  it("maps a centred iris to the middle with full confidence", () => {
    const g = estimateGaze({
      left: eye(0.3, 0.4, 0.05, { x: 0.3, y: 0.4 }),
      right: eye(0.7, 0.4, 0.05, { x: 0.7, y: 0.4 }),
    });
    expect(g.x).toBeCloseTo(0.5);
    expect(g.y).toBeCloseTo(0.5);
    expect(g.conf).toBe(1);
  });

  it("is monotonic: an iris toward the outer edge raises the coordinate", () => {
    const lookRight = estimateGaze({
      left: eye(0.3, 0.4, 0.05, { x: 0.34, y: 0.4 }),
      right: eye(0.7, 0.4, 0.05, { x: 0.74, y: 0.4 }),
    });
    expect(lookRight.x).toBeGreaterThan(0.5);
    const lookDown = estimateGaze({
      left: eye(0.3, 0.4, 0.05, { x: 0.3, y: 0.44 }),
      right: eye(0.7, 0.4, 0.05, { x: 0.7, y: 0.44 }),
    });
    expect(lookDown.y).toBeGreaterThan(0.5);
  });

  it("drops confidence when an eye is shut", () => {
    const shutLeft: Eye = {
      iris: { x: 0.3, y: 0.4 },
      left: { x: 0.25, y: 0.4 },
      right: { x: 0.35, y: 0.4 },
      top: { x: 0.3, y: 0.4 },
      bottom: { x: 0.3, y: 0.4 }, // top == bottom ⇒ closed
    };
    const g = estimateGaze({ left: shutLeft, right: eye(0.7, 0.4, 0.05, { x: 0.7, y: 0.4 }) });
    expect(g.conf).toBe(0.5);
  });

  it("clamps an iris outside its eye box into [0, 1]", () => {
    const g = estimateGaze({
      left: eye(0.3, 0.4, 0.05, { x: 0.9, y: 0.4 }),
      right: eye(0.7, 0.4, 0.05, { x: 0.9, y: 0.4 }),
    });
    expect(g.x).toBeLessThanOrEqual(1);
    expect(g.x).toBeGreaterThanOrEqual(0);
  });
});

describe("extractGazeLandmarks", () => {
  it("returns null without the refined (iris) mesh", () => {
    expect(extractGazeLandmarks([{ x: 0, y: 0 }])).toBeNull();
    expect(
      extractGazeLandmarks(Array.from({ length: 468 }, () => ({ x: 0.5, y: 0.5 }))),
    ).toBeNull();
  });

  it("picks the configured indices from a refined mesh", () => {
    const mesh: Point[] = Array.from({ length: 478 }, (_, i) => ({ x: i / 1000, y: i / 1000 }));
    const g = extractGazeLandmarks(mesh);
    expect(g).not.toBeNull();
    expect(g?.left.iris).toEqual(mesh[LANDMARK_INDEX.leftIris]);
    expect(g?.right.iris).toEqual(mesh[LANDMARK_INDEX.rightIris]);
  });
});
