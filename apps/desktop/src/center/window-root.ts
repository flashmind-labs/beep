import { createElement, Fragment, type ReactNode } from "react";
import { createDocumentScope } from "../ui/document-root.tsx";
import { RouterContextProvider, RouterProvider } from "@tanstack/react-router";
import { windowHistory } from "./window-router.ts";

type Part = "pages" | "mobile-tabs" | "mobile-back" | "mobile-keyboard" | "home" | "home-composer" | "sidebar" | "header-project-back" | "header-title" | "header-date" | "header-scope" | "header-tools" | "notices" | "usage" | "palette" | "project-shell" | "project-overview" | "project-runs" | "project-thread" | "review-detail" | "settings-sheet" | `settings-page:${"instructions" | "providers"}` | "project-sessions:all" | `git-history:${string}` | `thread-card:${string}` | `diff:${string}`;
const scope = createDocumentScope<Part>(children => createElement(RouterContextProvider, { router: windowHistory.router,
  children: createElement(Fragment, null, createElement(RouterProvider, { router: windowHistory.router }), children) }));

/// A window owns one tree. Stable portal hosts keep their DOM through unrelated updates.
export function showWindowParts(updates: readonly { part: Part; host: HTMLElement | null; content: ReactNode; identity?: string }[]) {
  scope.set(updates.map(({ part, ...update }) => ({ slot: part, ...update })));
}

export function removeWindowPart(part: Part) {
  removeWindowParts([part]);
}

export function removeWindowParts(removed: readonly Part[]) {
  scope.remove(removed);
}
