import { createElement, Fragment, type ReactNode } from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { createDocumentScope, documentScopes } from "../ui/document-root.tsx";
import { RouterContextProvider, RouterProvider } from "@tanstack/react-router";
import { windowHistory } from "./window-router.ts";
import { CenterWindow, shell, type ShellPart } from "./window-shell.tsx";

// The window's one React tree. It renders synchronously as this module loads, so the
// modules after it find the window's elements in place. Tests load modules without it.
const container = typeof document === "undefined" ? null : document.querySelector(".cwin");
if (container) {
  const root = createRoot(container);
  flushSync(() => root.render(createElement(RouterContextProvider, { router: windowHistory.router,
    children: createElement(Fragment, null, createElement(RouterProvider, { router: windowHistory.router }), createElement(CenterWindow), documentScopes()) })));
}

/// Fills places of the window, synchronously; null empties one.
export function placeWindowParts(parts: Partial<Record<ShellPart, ReactNode>>) {
  flushSync(() => shell.setState(s => ({ parts: { ...s.parts, ...parts } })));
}

type Part = "review-detail" | "mobile-keyboard" | "project-shell" | "project-sessions:all" | `settings-page:${"instructions" | "providers"}`
  | `git-history:${string}` | `thread-card:${string}` | `diff:${string}`;
const scope = createDocumentScope<Part>();

/// Draws into elements the window's views own. Stable hosts keep their DOM through unrelated updates.
export function showWindowParts(updates: readonly { part: Part; host: HTMLElement | null; content: ReactNode; identity?: string }[]) {
  scope.set(updates.map(({ part, ...update }) => ({ slot: part, ...update })));
}

export function removeWindowPart(part: Part) {
  removeWindowParts([part]);
}

export function removeWindowParts(removed: readonly Part[]) {
  scope.remove(removed);
}
