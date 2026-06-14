// SPDX-License-Identifier: AGPL-3.0-only

/**
 * Entry point and routing. The default view is the composer (paste a `control`
 * token, kept in `localStorage`); the `#care` hash opens the caregiver space
 * (a separate `care` token). Same-origin by construction — the hub serves these
 * files (PLAN 5.3); in dev, Vite proxies the hub.
 */

import { FluenceClient } from "@fluence/sdk";

import { CaregiverView } from "./caregiver.js";
import { Composer } from "./composer.js";
import { h } from "./dom.js";
import { t } from "./i18n.js";
import "./styles.css";

const TOKEN_KEY = "fluence.token";
const CARE_TOKEN_KEY = "fluence.care_token";

function appRoot(): HTMLElement {
  const element = document.getElementById("app");
  if (element === null) {
    throw new Error("missing #app root");
  }
  return element;
}

/** A connect screen, parameterized by which view it leads into. */
interface ConnectConfig {
  title: string;
  hint: string;
  storageKey: string;
  onConnect: () => void;
  error?: string;
}

function renderConnect(app: HTMLElement, config: ConnectConfig): void {
  const input = h("input", {
    class: "token-input",
    type: "password",
    autocomplete: "off",
    "aria-label": t("connect.tokenLabel"),
    placeholder: t("connect.tokenLabel"),
  });
  const submit = h("button", { class: "connect-submit", type: "button" }, [t("connect.submit")]);
  const submitToken = (): void => {
    const token = input.value.trim();
    if (token.length === 0) {
      return;
    }
    localStorage.setItem(config.storageKey, token);
    config.onConnect();
  };
  submit.addEventListener("click", submitToken);
  input.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      submitToken();
    }
  });

  app.replaceChildren(
    h("div", { class: "connect" }, [
      h("h1", {}, [config.title]),
      input,
      submit,
      h("p", { class: "connect-hint" }, [config.hint]),
      ...(config.error === undefined
        ? []
        : [h("p", { class: "connect-error", role: "alert" }, [config.error])]),
    ]),
  );
}

async function bootComposer(): Promise<void> {
  const app = appRoot();
  const stored = localStorage.getItem(TOKEN_KEY);
  if (stored === null) {
    renderConnect(app, {
      title: t("connect.title"),
      hint: t("connect.hint"),
      storageKey: TOKEN_KEY,
      onConnect: () => void bootComposer(),
    });
    return;
  }
  const client = new FluenceClient({ baseUrl: window.location.origin, token: stored });
  const composer = new Composer(client, app);
  try {
    await composer.start();
  } catch {
    localStorage.removeItem(TOKEN_KEY);
    renderConnect(app, {
      title: t("connect.title"),
      hint: t("connect.hint"),
      storageKey: TOKEN_KEY,
      onConnect: () => void bootComposer(),
      error: t("connect.error"),
    });
  }
}

async function bootCaregiver(): Promise<void> {
  const app = appRoot();
  const stored = localStorage.getItem(CARE_TOKEN_KEY);
  if (stored === null) {
    renderConnect(app, {
      title: t("connect.careTitle"),
      hint: t("connect.careHint"),
      storageKey: CARE_TOKEN_KEY,
      onConnect: () => void bootCaregiver(),
    });
    return;
  }
  const client = new FluenceClient({ baseUrl: window.location.origin, token: stored });
  const view = new CaregiverView(client, app);
  try {
    await view.start();
  } catch {
    localStorage.removeItem(CARE_TOKEN_KEY);
    renderConnect(app, {
      title: t("connect.careTitle"),
      hint: t("connect.careHint"),
      storageKey: CARE_TOKEN_KEY,
      onConnect: () => void bootCaregiver(),
      error: t("connect.error"),
    });
  }
}

function boot(): void {
  if (window.location.hash === "#care") {
    void bootCaregiver();
  } else {
    void bootComposer();
  }
}

window.addEventListener("hashchange", boot);
boot();
