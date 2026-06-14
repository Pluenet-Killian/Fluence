// SPDX-License-Identifier: AGPL-3.0-only
// @vitest-environment happy-dom

import type { AccessJournalEntry, DeviceInfo, HealthResponse } from "@fluence/sdk";
import { describe, expect, it, vi } from "vitest";

import { renderDevices, renderHealth, renderJournal } from "./caregiver.js";

describe("caregiver render helpers", () => {
  it("renders the hub version and each worker state", () => {
    const health: HealthResponse = {
      version: "0.0.0",
      started_at: "2026-06-14T10:00:00Z",
      workers: [
        { worker: "llm", state: "ready", restart_count: 0 },
        { worker: "tts", state: "down", restart_count: 3 },
      ],
      latencies: [],
    };
    const el = renderHealth(health);
    expect(el.textContent).toContain("v0.0.0");
    expect(el.textContent).toContain("llm — ready");
    expect(el.textContent).toContain("tts — down");
    expect(el.textContent).toContain("3×"); // restart count surfaced
  });

  it("revokes a device only after an explicit confirm (two clicks)", () => {
    const onRevoke = vi.fn();
    const devices: DeviceInfo[] = [
      {
        id: "dev-1",
        name: "tablette",
        kind: "tablet",
        scope: "control",
        created_at: "2026-06-14T10:00:00Z",
      },
    ];
    const el = renderDevices(devices, onRevoke);

    el.querySelector<HTMLButtonElement>(".care-revoke-btn")?.click();
    expect(onRevoke).not.toHaveBeenCalled(); // first click only arms the confirm

    el.querySelector<HTMLButtonElement>(".care-revoke-confirm")?.click();
    expect(onRevoke).toHaveBeenCalledTimes(1);
    expect(onRevoke).toHaveBeenCalledWith("dev-1");
  });

  it("can cancel an armed revoke", () => {
    const onRevoke = vi.fn();
    const el = renderDevices(
      [
        {
          id: "dev-1",
          name: "tablette",
          kind: "tablet",
          scope: "control",
          created_at: "2026-06-14T10:00:00Z",
        },
      ],
      onRevoke,
    );
    el.querySelector<HTMLButtonElement>(".care-revoke-btn")?.click();
    el.querySelector<HTMLButtonElement>(".care-revoke-cancel")?.click();
    expect(el.querySelector(".care-revoke-btn")).not.toBeNull(); // back to idle
    expect(onRevoke).not.toHaveBeenCalled();
  });

  it("lists a revoked device greyed, with no revoke button", () => {
    const devices: DeviceInfo[] = [
      {
        id: "dev-2",
        name: "vieux",
        kind: "desktop",
        scope: "care",
        created_at: "2026-06-14T10:00:00Z",
        revoked_at: "2026-06-14T11:00:00Z",
      },
    ];
    const el = renderDevices(devices, vi.fn());
    expect(el.querySelector(".care-revoked")).not.toBeNull();
    expect(el.querySelector(".care-revoke-btn")).toBeNull();
  });

  it("renders journal entries with their action and optional detail", () => {
    const entries: AccessJournalEntry[] = [
      { at: "2026-06-14T11:00:00Z", action: "device.revoked", detail: "id=dev-2" },
      { at: "2026-06-14T10:00:00Z", action: "device.paired" },
    ];
    const el = renderJournal(entries);
    expect(el.textContent).toContain("device.revoked");
    expect(el.textContent).toContain("id=dev-2");
    expect(el.textContent).toContain("device.paired");
  });

  it("shows empty states without crashing", () => {
    expect(renderDevices([], vi.fn()).querySelector(".care-muted")).not.toBeNull();
    expect(renderJournal([]).querySelector(".care-muted")).not.toBeNull();
  });
});
