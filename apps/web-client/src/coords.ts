// SPDX-License-Identifier: AGPL-3.0-only

/** A point normalized to `[0, 1]` within a surface (the input wire format). */
export interface NormalizedPoint {
  x: number;
  y: number;
}

/** The bounding box of the surface a pointer is normalized against. */
export interface SurfaceRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

function clamp01(value: number): number {
  return Math.min(1, Math.max(0, value));
}

/**
 * Maps a client-space point (e.g. `pointermove` clientX/Y) to `[0, 1]` within
 * `rect`, clamped — the normalized coordinates the hub's selection engine
 * expects (SPEC §4.A). A zero-sized surface maps to the origin.
 */
export function normalizePoint(
  clientX: number,
  clientY: number,
  rect: SurfaceRect,
): NormalizedPoint {
  const x = rect.width > 0 ? (clientX - rect.left) / rect.width : 0;
  const y = rect.height > 0 ? (clientY - rect.top) / rect.height : 0;
  return { x: clamp01(x), y: clamp01(y) };
}
