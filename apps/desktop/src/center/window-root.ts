import { createElement, Fragment, type ReactNode } from "react";
import { createPortal, flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { RouterProvider } from "@tanstack/react-router";
import { windowHistory } from "./window-router.ts";

type Part = "home" | "sidebar" | "header-scope" | "header-tools";
const parts = new Map<Part, { host: HTMLElement; content: ReactNode; identity: string }>();
let root: Root | null = null;

/// A window owns one tree. Stable portal hosts keep their DOM through unrelated updates.
export function showWindowParts(updates: readonly { part: Part; host: HTMLElement; content: ReactNode; identity?: string }[]) {
  for (const update of updates) parts.set(update.part, { host: update.host, content: update.content, identity: update.identity ?? update.part });
  root ??= createRoot(document.createElement("div"));
  const owner = root;
  flushSync(() => owner.render(createElement(Fragment, null,
    createElement(RouterProvider, { router: windowHistory.router, key: "router" }),
    ...[...parts].map(([part, value]) => createPortal(value.content, value.host, `${part}:${value.identity}`)))));
}
