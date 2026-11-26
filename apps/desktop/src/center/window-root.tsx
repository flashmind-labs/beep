import { Fragment, type ReactNode } from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { createDocumentScope, documentScopes, Guarded } from "../ui/document-root.tsx";
import { RouterContextProvider, RouterProvider } from "@tanstack/react-router";
import { windowHistory } from "./window-router.ts";
import { CenterWindow, shell, type ShellPart } from "./window-shell.tsx";

// The window's one React tree. It renders synchronously as this module loads, so the
// modules after it find the window's elements in place. Tests load modules without it.
const container = typeof document === "undefined" ? null : document.querySelector(".cwin");
if (container) {
  // A caught error fires no `error` event, so it is logged here with where it came from.
  // The page's bridge loads on demand: tests load this module without one.
  const root = createRoot(container, {
    onCaughtError: (error, info) => void import("../ui/tauri.ts").then(({ logError }) =>
      logError(`caught: ${error instanceof Error ? `${error.name}: ${error.message}` : String(error)}${info.componentStack ?? ""}`)),
  });
  flushSync(() => root.render(
    <RouterContextProvider router={windowHistory.router}>
      <Fragment><Guarded><RouterProvider router={windowHistory.router} /></Guarded><Guarded><CenterWindow /></Guarded>{documentScopes()}</Fragment>
    </RouterContextProvider>));
}

/// Fills places of the window, synchronously; null empties one.
export function placeWindowParts(parts: Partial<Record<ShellPart, ReactNode>>) {
  flushSync(() => shell.setState(s => ({ parts: { ...s.parts, ...parts } })));
}

type Part = "mobile-keyboard" | "project-shell" | "project-sessions:all" | "project-overview" | `settings-page:${"instructions" | "providers"}`
  | `git-history:${string}`;
const scope = createDocumentScope<Part>();

/// Draws into elements the window's views own. Stable hosts keep their DOM through unrelated updates.
export function showWindowParts(updates: readonly { part: Part; host: HTMLElement | null; content: ReactNode; identity?: string }[]) {
  scope.set(updates.map(({ part, ...update }) => ({ slot: part, ...update })));
}

export function removeWindowPart(part: Part) {
  removeWindowParts([part]);
}

function removeWindowParts(removed: readonly Part[]) {
  scope.remove(removed);
}
