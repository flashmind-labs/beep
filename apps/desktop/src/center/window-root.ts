import { createElement, Fragment, type ReactNode } from "react";
import { createPortal, flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { RouterContextProvider, RouterProvider } from "@tanstack/react-router";
import { windowHistory } from "./window-router.ts";

type Part = "home" | "sidebar" | "header-scope" | "header-tools" | "notices" | "usage" | "palette" | "project-shell" | "project-where" | "project-running" | "project-custom-agents" | "project-runs" | "project-thread" | "sync-page" | "research-list" | "research-open" | "review-status" | "review-heading" | "review-findings" | "review-actions" | `settings-page:${"instructions" | "providers"}` | `project-overview:${string}` | `project-sessions:${"all" | "recent"}` | `git-history:${string}` | `thread-card:${string}` | `diff:${string}`;
const parts = new Map<Part, { host: HTMLElement | null; content: ReactNode; identity: string }>();
let root: Root | null = null;

/// A window owns one tree. Stable portal hosts keep their DOM through unrelated updates.
export function showWindowParts(updates: readonly { part: Part; host: HTMLElement | null; content: ReactNode; identity?: string }[]) {
  for (const update of updates) parts.set(update.part, { host: update.host, content: update.content, identity: update.identity ?? update.part });
  render();
}

export function removeWindowPart(part: Part) {
  removeWindowParts([part]);
}

export function removeWindowParts(removed: readonly Part[]) {
  let changed = false;
  for (const part of removed) changed = parts.delete(part) || changed;
  if (changed) render();
}

function render() {
  root ??= createRoot(document.createElement("div"));
  const owner = root;
  const children = createElement(Fragment, null,
    createElement(RouterProvider, { router: windowHistory.router, key: "router" }),
    ...[...parts].map(([part, value]) => value.host
      ? createPortal(value.content, value.host, `${part}:${value.identity}`)
      : createElement(Fragment, { key: `${part}:${value.identity}` }, value.content)));
  flushSync(() => owner.render(createElement(RouterContextProvider, { router: windowHistory.router, children })));
}
