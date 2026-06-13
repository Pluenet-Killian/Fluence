// SPDX-License-Identifier: AGPL-3.0-only

/**
 * Entry point: a minimal connect screen (paste a `control` token, kept in
 * `localStorage`) then the composer. Same-origin by construction — the hub
 * serves these files (PLAN 5.3); in dev, Vite proxies the hub.
 */

import { FluenceClient } from "@fluence/sdk";

import { Composer } from "./composer.js";
import { h } from "./dom.js";
import { t } from "./i18n.js";
import "./styles.css";

const TOKEN_KEY = "fluence.token";

function appRoot(): HTMLElement {
  const element = document.getElementById("app");
  if (element === null) {
    throw new Error("missing #app root");
  }
  return element;
}

function renderConnect(app: HTMLElement, error?: string): void {
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
    localStorage.setItem(TOKEN_KEY, token);
    void boot();
  };
  submit.addEventListener("click", submitToken);
  input.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      submitToken();
    }
  });

  app.replaceChildren(
    h("div", { class: "connect" }, [
      h("h1", {}, [t("connect.title")]),
      input,
      submit,
      h("p", { class: "connect-hint" }, [t("connect.hint")]),
      ...(error === undefined ? [] : [h("p", { class: "connect-error", role: "alert" }, [error])]),
    ]),
  );
}

async function boot(): Promise<void> {
  const app = appRoot();
  const stored = localStorage.getItem(TOKEN_KEY);
  if (stored === null) {
    renderConnect(app);
    return;
  }
  const client = new FluenceClient({ baseUrl: window.location.origin, token: stored });
  const composer = new Composer(client, app);
  try {
    await composer.start();
  } catch {
    localStorage.removeItem(TOKEN_KEY);
    renderConnect(app, t("connect.error"));
  }
}

void boot();
