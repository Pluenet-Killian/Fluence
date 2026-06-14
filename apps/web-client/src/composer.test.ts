// SPDX-License-Identifier: AGPL-3.0-only

import { describe, expect, it } from "vitest";

import { SuggestionGate } from "./antiflicker.js";
import { normalizePoint } from "./coords.js";
import {
  allKeys,
  BACKSPACE,
  buildTargetMap,
  KEY_ROWS,
  targetsPatchFrame,
  type MeasuredKey,
} from "./keyboard.js";
import { UsageMeter } from "./metrics.js";

describe("normalizePoint", () => {
  const rect = { left: 100, top: 50, width: 200, height: 100 };

  it("maps a point to [0,1] within the surface", () => {
    expect(normalizePoint(200, 100, rect)).toEqual({ x: 0.5, y: 0.5 });
  });

  it("clamps points outside the surface", () => {
    expect(normalizePoint(0, 0, rect)).toEqual({ x: 0, y: 0 });
    expect(normalizePoint(1000, 1000, rect)).toEqual({ x: 1, y: 1 });
  });

  it("maps a zero-sized surface to the origin", () => {
    expect(normalizePoint(5, 5, { left: 0, top: 0, width: 0, height: 0 })).toEqual({ x: 0, y: 0 });
  });
});

describe("SuggestionGate (anti-flicker, SPEC §7.A)", () => {
  it("allows at most one update per interval", () => {
    const gate = new SuggestionGate(600, 0.4);
    expect(gate.allow(1000)).toBe(true);
    gate.mark(1000);
    expect(gate.allow(1300)).toBe(false); // 300 ms < 600 ms
    expect(gate.allow(1600)).toBe(true); // 600 ms elapsed
  });

  it("blocks updates while a dwell is past 40 %", () => {
    const gate = new SuggestionGate(600, 0.4);
    gate.setDwellProgress(0.5);
    expect(gate.allow(10_000)).toBe(false);
    gate.setDwellProgress(0.3);
    expect(gate.allow(10_000)).toBe(true);
  });
});

describe("keyboard layout", () => {
  it("has unique target ids and includes space + backspace", () => {
    const keys = allKeys();
    const ids = new Set(keys.map((key) => key.id));
    expect(ids.size).toBe(keys.length);
    expect(keys.find((key) => key.id === "key_space")?.output).toBe(" ");
    expect(keys.find((key) => key.id === "key_back")?.output).toBe(BACKSPACE);
  });

  it("covers the letter rows", () => {
    expect(KEY_ROWS.length).toBeGreaterThanOrEqual(3);
    expect(allKeys().some((key) => key.id === "key_e")).toBe(true);
  });

  it("builds a target map from measured rects", () => {
    const measured: MeasuredKey[] = [
      { id: "key_e", label: "e", role: "key", rect: [0, 0, 50, 50] },
    ];
    const map = buildTargetMap("main", { w: 800, h: 600 }, measured);
    expect(map.surface).toBe("main");
    expect(map.viewport).toEqual({ w: 800, h: 600 });
    expect(map.targets[0]?.id).toBe("key_e");
    expect(map.targets[0]?.rect).toEqual([0, 0, 50, 50]);
  });

  it("wraps a target map as an input targets.patch wire frame (live-engine seeding)", () => {
    const map = buildTargetMap("main", { w: 800, h: 600 }, [
      { id: "key_e", label: "e", role: "key", rect: [0, 0, 50, 50] },
    ]);
    const frame = targetsPatchFrame(map);
    // The exact wire shape the hub deserializes as InputClientMessage::TargetsPatch
    // (k tag, then apply_patch upserts these targets onto the live engine).
    expect(frame.topic).toBe("input");
    expect(frame.msg.k).toBe("targets.patch");
    expect(frame.msg.surface).toBe("main");
    expect(frame.msg.viewport).toEqual({ w: 800, h: 600 });
    expect(frame.msg.upsert).toEqual(map.targets);
    expect(frame.msg.remove).toEqual([]);
    // It must round-trip through JSON unchanged (sent via socket.send(JSON…)).
    expect(JSON.parse(JSON.stringify(frame))).toEqual(frame);
  });
});

describe("UsageMeter (instrumentation, PLAN 5.5)", () => {
  it("computes effective WPM from produced text over time", () => {
    const meter = new UsageMeter();
    meter.recordSelection(0);
    // 50 produced chars = 10 words, over one minute → 10 WPM.
    expect(meter.snapshot(50, 60_000).wpm).toBeCloseTo(10);
  });

  it("rewards suggestions over char-by-char typing in keystroke savings", () => {
    const accepted = new UsageMeter();
    accepted.recordSelection(0); // one selection drops a 20-char suggestion
    expect(accepted.snapshot(20, 1000).ksPercent).toBeCloseTo(95);

    const typed = new UsageMeter();
    Array.from({ length: 20 }).forEach((_, index) => {
      typed.recordSelection(index);
    });
    expect(typed.snapshot(20, 1000).ksPercent).toBeCloseTo(0);
  });
});
