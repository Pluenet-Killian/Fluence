// SPDX-License-Identifier: AGPL-3.0-only

import type { TargetMap } from "@fluence/sdk";

/** Backspace marker emitted by the erase key. */
export const BACKSPACE = "\b";

/** A keyboard key: its target id, label, what it types, and its role. */
export interface KeyDef {
  id: string;
  label: string;
  /** Appended to the draft when committed; [`BACKSPACE`] erases. */
  output: string;
  /** Dwell policy hint for the hub (SPEC §4.A). */
  role: "key" | "action";
}

function letters(row: string): KeyDef[] {
  return Array.from(row, (ch) => ({
    id: `key_${ch}`,
    label: ch,
    output: ch,
    role: "key" as const,
  }));
}

/** A compact AZERTY-adapted v0 layout: letter rows + space + backspace. */
export const KEY_ROWS: readonly (readonly KeyDef[])[] = [
  letters("azertyuiop"),
  letters("qsdfghjklm"),
  letters("wxcvbn"),
  [
    { id: "key_space", label: "Espace", output: " ", role: "action" },
    { id: "key_back", label: "⌫", output: BACKSPACE, role: "action" },
  ],
];

/** Every key, flattened, for id lookup. */
export function allKeys(): KeyDef[] {
  return KEY_ROWS.flatMap((row) => [...row]);
}

/** A key whose on-screen rectangle has been measured, in viewport pixels. */
export interface MeasuredKey {
  id: string;
  label: string;
  role: "key" | "action";
  rect: [number, number, number, number];
}

/**
 * Assembles the hub [`TargetMap`] from measured key rectangles (SPEC §4.A): the
 * UI declares its targets, the hub hit-tests and runs dwell.
 */
export function buildTargetMap(
  surface: string,
  viewport: { w: number; h: number },
  keys: readonly MeasuredKey[],
): TargetMap {
  return {
    surface,
    viewport,
    targets: keys.map((key) => ({
      id: key.id,
      rect: key.rect,
      role: key.role,
      label: key.label,
    })),
  };
}
