// SPDX-License-Identifier: AGPL-3.0-only

/**
 * The Fluence web composer (SPEC §7.A): a dwell-typing keyboard, three fixed
 * suggestion slots, an invariant PARLER button and a double-confirmed emergency,
 * wired to the hub through the SDK (WS selection events + SSE suggestions).
 *
 * Typing works two ways: a direct click (universal, testable) and hub-side dwell
 * (the accessibility path — pointer samples stream to the hub, which hit-tests
 * and runs the dwell timer, then emits selection events). Both funnel through
 * one `type()` so the draft logic is single-sourced.
 */

import { FluenceClient } from "@fluence/sdk";
import type {
  SelectionEvent,
  Suggestion,
  SuggestRequest,
  SystemEvent,
  TargetMap,
} from "@fluence/sdk";

import { SuggestionGate } from "./antiflicker.js";
import { h } from "./dom.js";
import { normalizePoint } from "./coords.js";
import { t } from "./i18n.js";
import {
  allKeys,
  BACKSPACE,
  buildTargetMap,
  KEY_ROWS,
  targetsPatchFrame,
  type MeasuredKey,
} from "./keyboard.js";
import { UsageMeter } from "./metrics.js";

const SURFACE = "main";
const SUGGESTION_SLOTS = 3;
const POINTER_THROTTLE_MS = 50;
const SUGGEST_DEBOUNCE_MS = 400;
const DRAFT_AUTOSAVE_MS = 500;
const RECONNECT_DELAY_MS = 1000;
const EMERGENCY_ARM_TIMEOUT_MS = 5000;
const DEFAULT_VOICE_ID = "piper:fr_FR-siwis-medium";

type Timer = ReturnType<typeof setTimeout>;

/** Drives one composer session against a connected hub. */
export class Composer {
  readonly #client: FluenceClient;
  readonly #root: HTMLElement;
  readonly #gate = new SuggestionGate();
  readonly #meter = new UsageMeter();
  readonly #keyEls = new Map<string, HTMLButtonElement>();

  #sessionId = "";
  #draft = "";
  #voiceId = DEFAULT_VOICE_ID;
  #focused: string | null = null;
  #emergencyArmed = false;

  #socketRaw: WebSocket | null = null;
  #closed = false;
  #suggestAbort: AbortController | null = null;
  #lastPointerMs = 0;
  #suggestTimer: Timer | null = null;
  #autosaveTimer: Timer | null = null;
  #emergencyTimer: Timer | null = null;
  #reconnectTimer: Timer | null = null;

  // DOM refs, assigned in render().
  #draftEl!: HTMLElement;
  #metricsEl!: HTMLElement;
  #statusEl!: HTMLElement;
  #bannerEl!: HTMLElement;
  #emergencyBtn!: HTMLButtonElement;
  #emergencyCancelBtn!: HTMLButtonElement;
  #suggestionEls: HTMLButtonElement[] = [];

  constructor(client: FluenceClient, root: HTMLElement) {
    this.#client = client;
    this.#root = root;
  }

  /**
   * Starts the session: opens a hub session, renders the UI, declares the
   * keyboard targets and connects the event socket.
   *
   * @throws when the hub rejects the session (e.g. an invalid token) — the
   * caller falls back to the connect screen.
   */
  async start(): Promise<void> {
    const session = await this.#client.createSession();
    this.#sessionId = session.session_id;
    this.#voiceId = await this.#pickVoice();
    this.#render();
    this.#declareTargets();
    this.#connect();
  }

  /** Picks a voice id: the first installed voice, else the Piper default. */
  async #pickVoice(): Promise<string> {
    try {
      const { voices } = await this.#client.voices();
      return voices[0]?.id ?? DEFAULT_VOICE_ID;
    } catch {
      return DEFAULT_VOICE_ID;
    }
  }

  #render(): void {
    this.#bannerEl = h("div", { class: "banner", role: "status", hidden: "" });
    this.#statusEl = h("div", { class: "status" }, [t("status.connected")]);
    this.#metricsEl = h("div", { class: "metrics", "aria-live": "polite" });
    this.#updateMetrics();
    this.#draftEl = h("output", { class: "draft", "aria-live": "polite" });
    this.#renderDraft();

    this.#suggestionEls = Array.from({ length: SUGGESTION_SLOTS }, () => {
      const slot = h("button", { class: "suggestion", type: "button" }, [t("suggest.slotEmpty")]);
      slot.addEventListener("click", () => {
        this.#acceptSuggestion(slot.dataset["text"] ?? "");
      });
      return slot;
    });
    const suggestions = h("div", { class: "suggestions" }, this.#suggestionEls);

    const keyboard = this.#renderKeyboard();

    const speakBtn = h("button", { class: "speak", type: "button" }, [t("compose.speak")]);
    speakBtn.addEventListener("click", () => {
      void this.#speak();
    });

    this.#emergencyBtn = h("button", { class: "emergency", type: "button" }, [
      t("compose.emergency"),
    ]);
    this.#emergencyBtn.addEventListener("click", () => {
      void this.#onEmergency();
    });
    this.#emergencyCancelBtn = h(
      "button",
      { class: "emergency-cancel", type: "button", hidden: "" },
      [t("compose.emergencyCancel")],
    );
    this.#emergencyCancelBtn.addEventListener("click", () => {
      this.#disarmEmergency();
    });
    const actions = h("div", { class: "actions" }, [
      speakBtn,
      this.#emergencyBtn,
      this.#emergencyCancelBtn,
    ]);

    this.#root.replaceChildren(
      this.#bannerEl,
      this.#statusEl,
      this.#draftEl,
      suggestions,
      keyboard,
      actions,
      this.#metricsEl,
    );
  }

  #renderKeyboard(): HTMLElement {
    this.#keyEls.clear();
    const rows = KEY_ROWS.map((row) =>
      h(
        "div",
        { class: "key-row" },
        row.map((key) => {
          const button = h(
            "button",
            { class: `key key-${key.role}`, type: "button", "data-id": key.id },
            [h("span", { class: "key-label" }, [key.label]), h("span", { class: "key-gauge" })],
          );
          button.addEventListener("click", () => {
            this.#type(key.output);
          });
          this.#keyEls.set(key.id, button);
          return button;
        }),
      ),
    );
    const keyboard = h("div", { class: "keyboard" }, rows);
    // Stream pointer samples to the hub so it runs hit-testing + dwell (D-4.1).
    keyboard.addEventListener("pointermove", (event) => {
      this.#onPointerMove(event, keyboard);
    });
    return keyboard;
  }

  // ---- Typing ----

  #type(output: string): void {
    if (output === BACKSPACE) {
      this.#draft = Array.from(this.#draft).slice(0, -1).join("");
    } else {
      this.#draft += output;
    }
    this.#meter.recordSelection(performance.now());
    this.#renderDraft();
    this.#updateMetrics();
    this.#scheduleAutosave();
    this.#scheduleSuggest();
  }

  #renderDraft(): void {
    this.#draftEl.textContent = this.#draft;
    this.#draftEl.classList.toggle("empty", this.#draft.length === 0);
    if (this.#draft.length === 0) {
      this.#draftEl.textContent = t("compose.draftPlaceholder");
    }
  }

  #acceptSuggestion(text: string): void {
    if (text.length === 0) {
      return;
    }
    this.#draft = text;
    this.#meter.recordSelection(performance.now());
    this.#renderDraft();
    this.#updateMetrics();
    this.#clearSuggestions();
    this.#scheduleAutosave();
  }

  #updateMetrics(): void {
    const { wpm, ksPercent } = this.#meter.snapshot(this.#draft.length, performance.now());
    this.#metricsEl.textContent = `${t("metrics.wpm")} ${wpm.toFixed(0)} · ${t("metrics.ks")} ${ksPercent.toFixed(0)} %`;
  }

  // ---- Targets ----

  #declareTargets(): void {
    const map = this.#measureTargets();
    if (map === null) {
      return;
    }
    // REST declaration: shared surface state, seeds the engine of any *future*
    // input connection from the hub's snapshot.
    void this.#client.putTargets(map).catch((error: unknown) => {
      console.warn("putTargets failed", error);
    });
    // WS declaration: seed *this* connection's live engine directly, in order,
    // over its own socket — deterministic even if the PUT above is still in
    // flight when the hub snapshots targets for this connection (PLAN 5.1).
    if (this.#socketRaw !== null && this.#socketRaw.readyState === WebSocket.OPEN) {
      this.#socketRaw.send(JSON.stringify(targetsPatchFrame(map)));
    }
  }

  /** Measures the on-screen key rectangles into a hub target map, or `null`
   * before the keyboard has been laid out. */
  #measureTargets(): TargetMap | null {
    const surface = this.#keyEls.size > 0 ? this.#root.querySelector(".keyboard") : null;
    if (surface === null) {
      return null;
    }
    const base = surface.getBoundingClientRect();
    const keys: MeasuredKey[] = [];
    for (const key of allKeys()) {
      const element = this.#keyEls.get(key.id);
      if (element === undefined) {
        continue;
      }
      const rect = element.getBoundingClientRect();
      keys.push({
        id: key.id,
        label: key.label,
        role: key.role,
        rect: [rect.left - base.left, rect.top - base.top, rect.width, rect.height],
      });
    }
    return buildTargetMap(SURFACE, { w: Math.round(base.width), h: Math.round(base.height) }, keys);
  }

  // ---- Socket ----

  #connect(): void {
    const socket = this.#client.socket(["input", "system"], {
      input: (event) => {
        this.#onSelection(event);
      },
      system: (event) => {
        this.#onSystem(event);
      },
    });
    this.#socketRaw = socket.raw;
    socket.raw.addEventListener("open", () => {
      this.#setStatus("status.connected");
      this.#declareTargets();
    });
    socket.raw.addEventListener("close", () => {
      this.#socketRaw = null;
      if (!this.#closed) {
        this.#setStatus("status.reconnecting");
        this.#reconnectTimer = setTimeout(() => {
          this.#reconnectTimer = null;
          this.#connect();
        }, RECONNECT_DELAY_MS);
      }
    });
  }

  /**
   * Stops the composer and releases its resources: cancels every timer and the
   * in-flight suggestion, and closes the socket without scheduling a reconnect.
   * Idempotent. Call before discarding the composer (e.g. on sign-out) so no
   * orphan timer keeps firing.
   */
  close(): void {
    this.#closed = true;
    for (const timer of [
      this.#suggestTimer,
      this.#autosaveTimer,
      this.#emergencyTimer,
      this.#reconnectTimer,
    ]) {
      if (timer !== null) {
        clearTimeout(timer);
      }
    }
    this.#suggestTimer = null;
    this.#autosaveTimer = null;
    this.#emergencyTimer = null;
    this.#reconnectTimer = null;
    this.#suggestAbort?.abort();
    this.#socketRaw?.close();
    this.#socketRaw = null;
  }

  #onSelection(event: SelectionEvent): void {
    switch (event.k) {
      case "sel.focus":
        this.#setFocus(event.target);
        break;
      case "sel.dwell":
        this.#setDwell(event.target, event.progress);
        break;
      case "sel.commit": {
        const key = allKeys().find((candidate) => candidate.id === event.target);
        if (key) {
          this.#type(key.output);
        }
        this.#clearFocus();
        break;
      }
      case "sel.cancel":
        this.#clearFocus();
        break;
      default:
        break;
    }
  }

  #onSystem(event: SystemEvent): void {
    switch (event.k) {
      case "system.emergency":
        this.#showBanner(
          event.active ? t("banner.emergencyActive") : t("banner.emergencyCleared"),
          event.active,
        );
        break;
      case "system.degraded":
        this.#setStatus("status.degraded");
        break;
      default:
        break;
    }
  }

  #setFocus(target: string): void {
    this.#clearFocus();
    this.#focused = target;
    this.#keyEls.get(target)?.classList.add("focused");
  }

  #setDwell(target: string, progress: number): void {
    this.#gate.setDwellProgress(progress);
    this.#keyEls.get(target)?.style.setProperty("--dwell", String(progress));
  }

  #clearFocus(): void {
    this.#gate.setDwellProgress(0);
    if (this.#focused !== null) {
      const element = this.#keyEls.get(this.#focused);
      element?.classList.remove("focused");
      element?.style.removeProperty("--dwell");
      this.#focused = null;
    }
  }

  // ---- Pointer streaming ----

  #onPointerMove(event: PointerEvent, surface: HTMLElement): void {
    const now = performance.now();
    if (now - this.#lastPointerMs < POINTER_THROTTLE_MS) {
      return;
    }
    this.#lastPointerMs = now;
    if (this.#socketRaw === null || this.#socketRaw.readyState !== WebSocket.OPEN) {
      return;
    }
    const point = normalizePoint(event.clientX, event.clientY, surface.getBoundingClientRect());
    // The generated `InputClientMessage` type drops the `k` tag (a contract-gen
    // quirk for newtype enum variants — tracked as debt); build the wire frame
    // explicitly, which is what the hub deserializes (`{topic, msg:{k:"ptr",…}}`).
    const frame = {
      topic: "input" as const,
      msg: {
        k: "ptr" as const,
        t: Math.round(now * 1000),
        src: "mouse:composer",
        x: point.x,
        y: point.y,
        conf: 1,
      },
    };
    this.#socketRaw.send(JSON.stringify(frame));
  }

  // ---- Suggestions ----

  #scheduleSuggest(): void {
    if (this.#suggestTimer !== null) {
      clearTimeout(this.#suggestTimer);
    }
    this.#suggestTimer = setTimeout(() => {
      void this.#requestSuggestions();
    }, SUGGEST_DEBOUNCE_MS);
  }

  async #requestSuggestions(): Promise<void> {
    const draft = this.#draft;
    if (draft.trim().length === 0) {
      this.#clearSuggestions();
      return;
    }
    this.#suggestAbort?.abort();
    const abort = new AbortController();
    this.#suggestAbort = abort;
    const request = {
      mode: "rephrase",
      draft,
      n: SUGGESTION_SLOTS,
      slot: "main",
    } satisfies SuggestRequest;
    try {
      let texts: string[] = [];
      for await (const event of this.#client.suggest(this.#sessionId, request, abort.signal)) {
        if (event.event === "final") {
          const data = event.data as { suggestions?: Suggestion[] };
          texts = (data.suggestions ?? []).map((suggestion) => suggestion.text);
        }
      }
      this.#applySuggestions(texts);
    } catch (error) {
      if (!abort.signal.aborted) {
        console.warn("suggest failed", error);
      }
    }
  }

  #applySuggestions(texts: string[]): void {
    const now = performance.now();
    if (!this.#gate.allow(now)) {
      return; // anti-flicker: too soon, or a dwell is in progress (SPEC §7.A)
    }
    this.#gate.mark(now);
    this.#suggestionEls.forEach((element, index) => {
      const text = texts[index] ?? "";
      element.dataset["text"] = text;
      element.textContent = text.length > 0 ? text : t("suggest.slotEmpty");
    });
  }

  #clearSuggestions(): void {
    this.#suggestionEls.forEach((element) => {
      element.dataset["text"] = "";
      element.textContent = t("suggest.slotEmpty");
    });
  }

  // ---- Draft autosave ----

  #scheduleAutosave(): void {
    if (this.#autosaveTimer !== null) {
      clearTimeout(this.#autosaveTimer);
    }
    this.#autosaveTimer = setTimeout(() => {
      void this.#client
        .putDraft(this.#sessionId, { text: this.#draft, caret: this.#draft.length })
        .catch((error: unknown) => {
          console.warn("draft autosave failed", error);
        });
    }, DRAFT_AUTOSAVE_MS);
  }

  // ---- Speak ----

  async #speak(): Promise<void> {
    if (this.#draft.trim().length === 0) {
      return;
    }
    try {
      const response = await this.#client.speak({ text: this.#draft, voice_id: this.#voiceId });
      const blob = await response.blob();
      const url = URL.createObjectURL(blob);
      const audio = new Audio(url);
      audio.addEventListener("ended", () => {
        URL.revokeObjectURL(url);
      });
      await audio.play();
    } catch (error) {
      console.warn("speak failed", error);
    }
  }

  // ---- Emergency (double confirmation, SPEC §7.A) ----

  async #onEmergency(): Promise<void> {
    if (!this.#emergencyArmed) {
      this.#armEmergency();
      return;
    }
    // Send first, disarm only on success: if the hub call fails this is a
    // critical action, so keep it armed for an immediate retry and surface the
    // failure (never silently swallow it, never pretend it was sent).
    try {
      await this.#client.emergency(true);
      this.#disarmEmergency();
    } catch (error) {
      console.warn("emergency send failed", error instanceof Error ? error.message : error);
      this.#showBanner(t("banner.emergencyFailed"), false);
    }
  }

  #armEmergency(): void {
    this.#emergencyArmed = true;
    this.#emergencyBtn.textContent = t("compose.emergencyConfirm");
    this.#emergencyBtn.classList.add("armed");
    this.#emergencyCancelBtn.hidden = false;
    this.#emergencyTimer = setTimeout(() => {
      this.#disarmEmergency();
    }, EMERGENCY_ARM_TIMEOUT_MS);
  }

  #disarmEmergency(): void {
    this.#emergencyArmed = false;
    this.#emergencyBtn.textContent = t("compose.emergency");
    this.#emergencyBtn.classList.remove("armed");
    this.#emergencyCancelBtn.hidden = true;
    if (this.#emergencyTimer !== null) {
      clearTimeout(this.#emergencyTimer);
      this.#emergencyTimer = null;
    }
  }

  // ---- Banner / status ----

  #showBanner(text: string, active: boolean): void {
    this.#bannerEl.textContent = text;
    this.#bannerEl.hidden = false;
    this.#bannerEl.classList.toggle("active", active);
    if (active) {
      void this.#ring();
    }
  }

  /** A short local ring tone for an emergency (Web Audio, no asset needed). */
  async #ring(): Promise<void> {
    try {
      const context = new AudioContext();
      const oscillator = context.createOscillator();
      oscillator.frequency.value = 880;
      oscillator.connect(context.destination);
      oscillator.start();
      await new Promise((resolve) => setTimeout(resolve, 600));
      oscillator.stop();
      await context.close();
    } catch {
      // Audio may be blocked before a user gesture; the banner still shows.
    }
  }

  #setStatus(key: Parameters<typeof t>[0]): void {
    this.#statusEl.textContent = t(key);
    this.#statusEl.dataset["state"] = key;
  }
}
