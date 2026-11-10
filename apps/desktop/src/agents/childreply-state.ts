import { esc } from "../ui/format.ts";
import type { PromptPart } from "./promptparts-state.ts";

type ChildPart = Extract<PromptPart, { kind: "child" }>;

/// The body and icon are HTML made by the desktop's renderers.
export function childReplyHtml(part: ChildPart, body: string, icon: string, meta: string) {
  const title = part.name || "Child agent";
  const state = part.state === "asking" ? "Waiting on you" : part.state === "closed" ? "Session ended" : part.state === "stopped" ? "Stopped" : "Done";
  return `<div class="dcoderen kidreply" role="group" aria-label="Reply from ${esc(title)}">`
    + `<button class="kh" type="button" data-kid="${esc(part.id)}" title="Open ${esc(title)}">${icon}<b>${esc(title)}</b><span class="m">${esc(meta)}</span><span class="ks">${state}</span></button>`
    + `<div class="dcoderen agent md">${body}</div></div>`;
}
