// SPDX-License-Identifier: AGPL-3.0-only

/** Minimal typed DOM helper: create an element with attributes and children. */
export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs: Partial<Record<string, string>> = {},
  children: (Node | string)[] = [],
): HTMLElementTagNameMap[K] {
  const element = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs)) {
    if (value !== undefined) {
      element.setAttribute(key, value);
    }
  }
  for (const child of children) {
    element.append(typeof child === "string" ? document.createTextNode(child) : child);
  }
  return element;
}

/** Replaces every child of `parent` with `children`. */
export function replaceChildren(parent: Element, ...children: Node[]): void {
  parent.replaceChildren(...children);
}
