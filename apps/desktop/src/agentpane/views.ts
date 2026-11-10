// What each kind of tab shows: text, a terminal, a picture, a video, a PDF
// or a 3D model.
import { resolvePath } from "./media-state.ts";
import { TextView } from "./text-view.tsx";
import { ImageView } from "./image-view.tsx";
import { drawPending, ModelView, PdfView, VideoView, type ModelState } from "./media-view.tsx";
import { createElement, type ReactNode } from "react";
import { setSessionPortal } from "../agents/session-frame.tsx";
import type { PaneTerm } from "./terminal.ts";
import { type TreeText, type MediaTab, P } from "./media.ts";
import { readFile, drawTermStatus } from "./read.ts";
import { h } from "../ui/dom.ts";

export function viewOf(t: MediaTab): HTMLElement {
  if (t.error || (t.kind === "text" && !t.text) || (t.kind === "term" && !t.term) || (!t.url && t.kind !== "text" && t.kind !== "term")) return pendingView(t);

  if (t.kind === "text" && t.text) return textView(t, t.text);
  if (t.kind === "term" && t.term) return termView(t, t.term);
  if (t.kind === "image") return imageView(t);
  if (t.kind === "video") return videoView(t);
  if (t.kind === "pdf") return pdfView(t);
  return t.bytes ? modelView(t, t.bytes) : pendingView(t);
}

/// A tab still reading, or one that failed. `read.ts` draws its progress.
function pendingView(t: MediaTab) {
  const el = h('<div class="agpview"></div>');
  drawPending(el, t, P.session?.machine_name ?? null);
  return el;
}

/// A file of the Files tab as text: numbered lines, lightly highlighted,
/// read only. Only its start shows when it is large.
function textView(t: MediaTab, text: TreeText) {
  return host(createElement(TextView, { tab: t, text }));
}

/// A terminal tab: its live screen, and a footer to attach again or stop it.
/// xterm owns the screen, so it stays a native child of the host.
function termView(t: MediaTab, term: PaneTerm) {
  const el = h('<div class="agpview term"></div>');
  el.prepend(term.screen.el);
  queueMicrotask(() => drawTermStatus(t));
  return el;
}

/// The image transform stays on the native image while React owns its controls.
function imageView(t: MediaTab) {
  return host(createElement(ImageView, { tab: t, machine: P.session?.machine_id ?? null }));
}

function videoView(t: MediaTab) {
  return host(createElement(VideoView, { tab: t, machine: P.session?.machine_id ?? null }));
}

function pdfView(t: MediaTab) {
  return host(createElement(PdfView, { tab: t, machine: P.session?.machine_id ?? null }));
}

/// The pane keeps each tab's host; the session frame renders into it.
function host(view: ReactNode) {
  const el = h('<div class="agpview"></div>');
  setSessionPortal(el, view);
  return el;
}

/// A 3D model: Three.js draws into the stage React renders, once per tab.
function modelView(t: MediaTab, bytes: ArrayBuffer) {
  const machine = P.session?.machine_id ?? null;
  let started = false;
  const render = (state: ModelState) => setSessionPortal(el, createElement(ModelView, { tab: t, machine, state, stage }));
  const stage = (box: HTMLDivElement | null) => {
    if (!box || started) return;
    started = true;
    void draw(box);
  };
  const draw = async (box: HTMLElement) => {
    try {
      const { mountModel } = await import("./model3d.ts");
      const view = await mountModel(box, bytes, t.name.slice(t.name.lastIndexOf(".") + 1).toLowerCase(), sibling);
      if (!P.tabs.includes(t) || t.el !== el) return view.dispose();
      t.model = view;
      render({ kind: "drawn" });
      view.resize();
    } catch (e) {
      if (P.tabs.includes(t) && t.el === el) render({ kind: "failed", message: e instanceof Error ? e.message : String(e) });
    }
  };
  // A .gltf's buffers and textures are files beside it.
  const dir = t.path ? t.path.slice(0, t.path.lastIndexOf("/") + 1) : "";
  const sibling = async (uri: string) => {
    if (!P.session || !dir || /^[a-z]+:/i.test(uri)) return null;

    try {
      return (await readFile(P.session.machine_id, resolvePath(dir, decodeURIComponent(uri)), () => {})).bytes;
    } catch {
      return null;
    }
  };

  const el = h('<div class="agpview"></div>');
  render({ kind: "drawing" });
  return el;
}
