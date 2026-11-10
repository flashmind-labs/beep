// What each kind of tab shows: text, a terminal, a picture, a video, a PDF
// or a 3D model.
import { resolvePath } from "./media-state.ts";
import { TextView } from "./text-view.tsx";
import { ImageView } from "./image-view.tsx";
import { ModelFooter, PdfView, VideoView } from "./media-view.tsx";
import { createElement } from "react";
import { removeSessionPortal, setSessionPortal } from "../agents/session-frame.tsx";
import { ModelControls } from "./model-controls.tsx";
import type { PaneTerm } from "./terminal.ts";
import { type TreeText, type MediaTab, P } from "./media.ts";
import { readFile, drawTermStatus } from "./read.ts";
import { esc } from "../ui/format.ts";
import { h } from "../ui/dom.ts";

export function viewOf(t: MediaTab): HTMLElement {
  if (t.error) return h(`<div class="agpview"><div class="agpmsg derr">${esc(t.error)}</div></div>`);
  if (t.kind === "text") return t.text ? textView(t, t.text) : h(`<div class="agpview"><div class="agpmsg"><span class="dspin"></span>Reading ${esc(t.name)} from ${esc(P.session?.machine_name ?? "the machine")}…</div></div>`);
  if (t.kind === "term") return t.term ? termView(t, t.term) : h(`<div class="agpview"><div class="agpmsg"><span class="dspin"></span>Opening a terminal on ${esc((t.at ?? t.existing)?.machine_name ?? "the machine")}…</div></div>`);
  if (!t.url) return h(`<div class="agpview"><div class="agpmsg"><span class="dspin"></span>Opening ${esc(t.name)}…<span class="agpload"><i></i></span></div></div>`);
  if (t.kind === "image") return imageView(t);
  if (t.kind === "video") return videoView(t);
  if (t.kind === "pdf") return pdfView(t);
  return modelView(t);
}

/// A file of the Files tab as text: numbered lines, lightly highlighted,
/// read only. Only its start shows when it is large.
function textView(t: MediaTab, text: TreeText) {
  const el = h('<div class="agpview"></div>');
  setSessionPortal(el, createElement(TextView, { tab: t, text }));
  return el;
}

/// A terminal tab: its live screen, and a footer to attach again or stop it.
function termView(t: MediaTab, term: PaneTerm) {
  const el = h('<div class="agpview term"></div>');
  el.prepend(term.screen.el);
  queueMicrotask(() => drawTermStatus(t));
  return el;
}

/// The image transform stays on the native image while React owns its controls.
function imageView(t: MediaTab) {
  const el = h('<div class="agpview"></div>');
  setSessionPortal(el, createElement(ImageView, { tab: t, machine: P.session?.machine_id ?? null }));
  return el;
}

function videoView(t: MediaTab) {
  const el = h('<div class="agpview"></div>');
  setSessionPortal(el, createElement(VideoView, { tab: t, machine: P.session?.machine_id ?? null }));
  return el;
}

function pdfView(t: MediaTab) {
  const el = h('<div class="agpview"></div>');
  setSessionPortal(el, createElement(PdfView, { tab: t, machine: P.session?.machine_id ?? null }));
  return el;
}

function modelView(t: MediaTab) {
  const el = h(`<div class="agpview"><div class="agpstage model"></div>
    <div class="agpacts anim" hidden></div>
    <div class="agpacts model-actions"></div></div>`);
  const stage = el.querySelector<HTMLElement>(".agpstage")!;
  const actions = el.querySelector<HTMLElement>(".model-actions")!;
  const machine = P.session?.machine_id ?? null;
  const drawActions = () => setSessionPortal(actions, createElement(ModelFooter, { tab: t, machine }));
  drawActions();
  setSessionPortal(stage, createElement("div", { className: "agpmsg" }, createElement("span", { className: "dspin" }), `Drawing ${t.name}…`));
  const dir = t.path ? t.path.slice(0, t.path.lastIndexOf("/") + 1) : "";
  // A .gltf's buffers and textures are files beside it.
  const sibling = async (uri: string) => {
    if (!P.session || !dir || /^[a-z]+:/i.test(uri)) return null;

    try {
      return (await readFile(P.session.machine_id, resolvePath(dir, decodeURIComponent(uri)), () => {})).bytes;
    } catch {
      return null;
    }
  };
  import("./model3d.ts")
    .then(m => m.mountModel(stage, t.bytes!, t.name.slice(t.name.lastIndexOf(".") + 1).toLowerCase(), sibling))
    .then(view => {
      if (!P.tabs.includes(t) || t.el !== el) return view.dispose();
      t.model = view;
      removeSessionPortal(stage);
      drawActions();
      if (view.anim) {
        const controls = el.querySelector<HTMLElement>(".anim");
        if (controls) { setSessionPortal(controls, createElement(ModelControls, { animation: view.anim })); controls.hidden = false; }
      }
      view.resize();
    })
    .catch(e => {
      if (!P.tabs.includes(t) || t.el !== el) return;
      removeSessionPortal(stage);
      stage.replaceChildren();
      setSessionPortal(stage, createElement("div", { className: "agpmsg derr" }, `Could not draw ${t.name}: ${e instanceof Error ? e.message : String(e)}`));
    });
  return el;
}
