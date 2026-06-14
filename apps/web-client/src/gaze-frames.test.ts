// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from "vitest";

import { calibrationFitFrame, calibrationSampleFrame, gazePointerFrame } from "./gaze-frames.js";

describe("gaze wire frames", () => {
  it("builds a clamped gaze pointer frame", () => {
    const frame = gazePointerFrame("gaze:webcam", 1.5, -0.2, 0.8, 123);
    expect(frame).toEqual({
      topic: "input",
      msg: { k: "ptr", t: 123, src: "gaze:webcam", x: 1, y: 0, conf: 0.8 },
    });
    expect(JSON.parse(JSON.stringify(frame))).toEqual(frame);
  });

  it("builds a calibration sample frame", () => {
    const frame = calibrationSampleFrame("main", "key_e", 0.4, 0.6);
    expect(frame.msg).toEqual({
      k: "cal.sample",
      surface: "main",
      target: "key_e",
      x: 0.4,
      y: 0.6,
    });
  });

  it("builds a calibration fit frame", () => {
    expect(calibrationFitFrame("main")).toEqual({
      topic: "input",
      msg: { k: "cal.fit", surface: "main" },
    });
  });
});
