// SPDX-License-Identifier: AGPL-3.0-only

/**
 * Caregiver space (SPEC §7.C, PLAN 7.2): a read-mostly dashboard for Sophie and
 * Jean — system health, paired devices (with revocation), and the access
 * journal. **Care scope only**: it never reaches P0 conversation content (that
 * is `control`), and the journal it shows is metadata, never user content.
 *
 * The render helpers are pure (data → DOM) so they unit-test without a hub.
 */

import type { AccessJournalEntry, DeviceInfo, FluenceClient, HealthResponse } from "@fluence/sdk";

import { h } from "./dom.js";
import { t } from "./i18n.js";

/** Formats an ISO timestamp for display, tolerating a malformed value. */
function formatTime(iso: string): string {
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? iso : date.toLocaleString();
}

/** System-health card: hub version and one line per supervised worker. */
export function renderHealth(health: HealthResponse): HTMLElement {
  const workers =
    health.workers.length === 0
      ? [h("li", { class: "care-muted" }, [t("care.noWorkers")])]
      : health.workers.map((worker) =>
          h("li", { class: `care-worker care-state-${worker.state}` }, [
            `${worker.worker} — ${worker.state}`,
            ...(worker.restart_count > 0
              ? [h("span", { class: "care-restart" }, [` (${String(worker.restart_count)}×)`])]
              : []),
          ]),
        );
  return h("section", { class: "care-card", "aria-label": t("care.health") }, [
    h("h2", {}, [t("care.health")]),
    h("p", { class: "care-version" }, [`v${health.version}`]),
    h("ul", { class: "care-workers" }, workers),
  ]);
}

/** Paired-devices card: each device with a (double-click) revoke button; a
 * revoked device stays listed, greyed, with no button. */
export function renderDevices(
  devices: DeviceInfo[],
  onRevoke: (deviceId: string) => void,
): HTMLElement {
  const rows = devices.map((device) => {
    const meta = h("span", { class: "care-device-meta" }, [
      `${device.name} · ${device.kind} · ${device.scope}`,
    ]);
    if (device.revoked_at != null) {
      return h("li", { class: "care-device care-revoked" }, [
        meta,
        h("span", { class: "care-revoked-tag" }, [t("care.revoked")]),
      ]);
    }
    return h("li", { class: "care-device" }, [meta, revokeButton(device.id, onRevoke)]);
  });
  return h("section", { class: "care-card", "aria-label": t("care.devices") }, [
    h("h2", {}, [t("care.devices")]),
    devices.length === 0
      ? h("p", { class: "care-muted" }, [t("care.noDevices")])
      : h("ul", { class: "care-devices" }, rows),
  ]);
}

/** A two-click revoke control (cutting off a device is consequential, so the
 * first click arms a confirm/cancel pair rather than acting immediately). */
function revokeButton(deviceId: string, onRevoke: (deviceId: string) => void): HTMLElement {
  const wrap = h("span", { class: "care-revoke" });
  const render = (armed: boolean): void => {
    if (!armed) {
      const button = h("button", { class: "care-revoke-btn", type: "button" }, [t("care.revoke")]);
      button.addEventListener("click", () => {
        render(true);
      });
      wrap.replaceChildren(button);
      return;
    }
    const confirm = h("button", { class: "care-revoke-confirm", type: "button" }, [
      t("care.revokeConfirm"),
    ]);
    const cancel = h("button", { class: "care-revoke-cancel", type: "button" }, [t("care.cancel")]);
    confirm.addEventListener("click", () => {
      onRevoke(deviceId);
    });
    cancel.addEventListener("click", () => {
      render(false);
    });
    wrap.replaceChildren(confirm, cancel);
  };
  render(false);
  return wrap;
}

/** Access-journal card: recent entries, newest first (metadata only). */
export function renderJournal(entries: AccessJournalEntry[]): HTMLElement {
  const rows =
    entries.length === 0
      ? [h("li", { class: "care-muted" }, [t("care.noJournal")])]
      : entries.map((entry) =>
          h("li", { class: "care-journal-entry" }, [
            h("span", { class: "care-journal-at" }, [formatTime(entry.at)]),
            h("span", { class: "care-journal-action" }, [` ${entry.action}`]),
            ...(entry.detail == null
              ? []
              : [h("span", { class: "care-journal-detail" }, [` — ${entry.detail}`])]),
          ]),
        );
  return h("section", { class: "care-card", "aria-label": t("care.journal") }, [
    h("h2", {}, [t("care.journal")]),
    h("ul", { class: "care-journal" }, rows),
  ]);
}

/** The live caregiver dashboard: fetches, renders, and refreshes on revoke. */
export class CaregiverView {
  readonly #client: FluenceClient;
  readonly #root: HTMLElement;

  constructor(client: FluenceClient, root: HTMLElement) {
    this.#client = client;
    this.#root = root;
  }

  /** Fetches the caregiver data and renders the dashboard. */
  async start(): Promise<void> {
    await this.#refresh();
  }

  async #refresh(): Promise<void> {
    const [health, journal, devices] = await Promise.all([
      this.#client.health(),
      this.#client.journal(50),
      this.#client.devices(),
    ]);
    this.#root.replaceChildren(
      h("div", { class: "care" }, [
        h("h1", {}, [t("care.title")]),
        renderHealth(health),
        renderDevices(devices.devices, (id) => {
          void this.#revoke(id);
        }),
        renderJournal(journal.entries),
      ]),
    );
  }

  async #revoke(deviceId: string): Promise<void> {
    await this.#client.revokeDevice(deviceId);
    await this.#refresh();
  }
}
