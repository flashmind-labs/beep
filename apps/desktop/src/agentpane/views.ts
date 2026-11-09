// What each kind of tab shows: text, a terminal, a picture, a video, a PDF
// or a 3D model.
import { isHtmlFile } from "./html-preview-state.ts";
import { actualView, baseName, fitView, resolvePath, zoomAt } from "./media-state.ts";
import { highlight, languageOf, MAX_LINES, shortSize } from "./files-state.ts";
import { createElement } from "react";
import { setSessionPortal } from "../agents/session-frame.tsx";
import { ModelControls } from "./model-controls.tsx";
import { fileSize, openViewer } from "../chat/media.ts";
import { attach, detachTerm, killTerm, type PaneTerm } from "./terminal.ts";
import { confirmSheet, openMenu, type MenuItem } from "../ui/menu.ts";
import { addMention } from "../agents/workdrop.ts";
import { type TreeText, type MediaTab, P, closeTab, saveWorkFile } from "./media.ts";
import { mentioned } from "./worktree-state.ts";
import { readFile, readError, drawTermStatus } from "./read.ts";
import { esc } from "../ui/format.ts";
import { h } from "../ui/dom.ts";
import { invoke } from "../ui/tauri.ts";

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
  const rel = t.rel!;
  const lang = languageOf(rel);
  let body: string;
  let lines = 0;

  if (text.binary) {
    body = `<div class="agpmsg">${esc(t.name)} is not text (${esc(shortSize(text.size))}). Save it to open it on this computer.</div>`;
  } else {
    const all = text.text.split("\n");
    if (all.length > 1 && all.at(-1) === "") all.pop();
    lines = all.length;
    const shown = all.slice(0, MAX_LINES);
    const width = String(shown.length).length;
    body = `<div class="agcode" style="--lnw:${width}ch"><pre>${shown.map((l, i) => `<span class="ln">${i + 1}</span>${highlight(l, lang)}`).join("\n")}</pre></div>`;
  }

  if (t.preview?.active) {
    body = t.preview.loading ? '<div class="agpmsg">Loading preview…</div>'
      : t.preview.error ? `<div class="agpmsg derr">${esc(t.preview.error)}</div>`
      : '<div class="agartframe-box"><iframe class="agartframe" sandbox="allow-scripts" referrerpolicy="no-referrer"></iframe></div>';
  }
  const cut = text.cut || lines > MAX_LINES ? ` · the first ${lines > MAX_LINES ? `${MAX_LINES.toLocaleString()} lines` : shortSize(text.text.length)} shown` : "";
  const about = [rel, lang, text.binary ? "" : `${lines.toLocaleString()} lines`, shortSize(text.size)].filter(Boolean).join(" · ") + cut;
  const el = h(`<div class="agpview">${body}<div class="agpacts"><span class="meta mono" title="${esc(rel)}">${esc(about)}</span><span class="sp"></span>${!text.binary && isHtmlFile(rel) ? `<button class="dbtn" data-preview="1">${t.preview?.active ? "Source" : "Preview"}</button>` : ""}<button class="dbtn" data-tment="1">Mention</button><button class="dbtn" data-tsave="1">Save</button></div></div>`);
  const frame = el.querySelector("iframe");
  if (frame && t.preview?.src) { frame.title = `Preview of ${t.name}`; frame.src = t.preview.src; }
  const render = () => {
    if (!P.tabs.includes(t) || !t.el) return;
    const next = textView(t, text);
    next.hidden = t.el.hidden;
    t.el.replaceWith(next);
    t.el = next;
  };
  el.querySelector("[data-preview]")?.addEventListener("click", async () => {
    const s = P.session;
    if (!s) return;
    const preview = t.preview ??= { active: false, loading: false, src: "", error: "" };
    preview.active = !preview.active;
    if (!preview.active || preview.src || preview.loading) { render(); return; }
    preview.loading = true;
    preview.error = "";
    render();

    try {
      const got = await invoke<{ kind: string; src: string }>("agent_artifact", {
        machineId: s.machine_id, root: t.root ?? s.cwd, path: rel, preview: true,
      });
      if (got.kind !== "html") throw new Error("This file cannot be previewed as HTML.");
      preview.src = got.src;
    } catch (e) {
      preview.error = readError(e);
    } finally {
      preview.loading = false;
      render();
    }
  });
  const meta = el.querySelector(".meta")!;
  el.querySelector("[data-tment]")!.addEventListener("click", () => addMention(mentioned(t.root ?? P.session?.cwd ?? "", P.session?.cwd ?? "", rel)));
  el.querySelector("[data-tsave]")!.addEventListener("click", () => saveWorkFile(rel, note => { meta.textContent = note; }, t.root));
  return el;
}

/// A terminal tab: its live screen, and a footer to attach again or stop it.
function termView(t: MediaTab, term: PaneTerm) {
  const el = h(`<div class="agpview term"><div class="agpacts"><span class="meta mono"></span><span class="sp"></span><button class="dbtn" data-tattach="1" hidden>Attach</button><button class="dbtn warn" data-tkill="1">Close</button></div></div>`);
  el.prepend(term.screen.el);
  const reattach = () => { term.tries = 0; attach(term); };
  const kill = () => killTerm(term).then(() => { t.term = undefined; closeTab(t.id); });
  const close = () => {
    if (term.row.state !== "running" || term.link === "ended") {
      kill().catch(e => { el.querySelector(".meta")!.textContent = readError(e); });
      return;
    }

    confirmSheet({
      title: `Stop ${t.name}?`,
      text: `It stops running on ${term.row.machine_name}.`,
      action: "Stop",
      run: kill,
    });
  };
  el.querySelector("[data-tattach]")!.addEventListener("click", reattach);
  el.querySelector("[data-tkill]")!.addEventListener("click", close);
  el.querySelector(".agpacts")!.addEventListener("contextmenu", e => {
    const items: MenuItem[] = [];
    if (term.link === "detached") items.push({ label: "Attach", on: reattach });
    if (term.link === "attached" || term.link === "attaching") items.push({ label: "Detach", on: () => detachTerm(term) });
    items.push({ label: term.row.state === "running" && term.link !== "ended" ? "Kill…" : "Close", warn: true, on: close });
    openMenu(e as MouseEvent, t.name, items);
  });
  queueMicrotask(() => drawTermStatus(t));
  return el;
}

/// The buttons that act on a file of the machine: open it in the Mac's
/// own app, save it to Downloads.
function fileButtons(t: MediaTab, openLabel: string) {
  if (!t.path) return "";
  return `<button class="dbtn" data-mopen="1">${esc(openLabel)}</button><button class="dbtn" data-msave="1">Save</button>`;
}

function wireFileButtons(t: MediaTab, el: HTMLElement) {
  const s = P.session;
  if (!s || !t.path) return;
  const note = (text: string) => { const m = el.querySelector(".agpacts .meta"); if (m) m.textContent = text; };
  el.querySelector("[data-mopen]")?.addEventListener("click", () => {
    invoke("agent_file_open", { machineId: s.machine_id, path: t.path }).catch(e => note(readError(e)));
  });
  el.querySelector("[data-msave]")?.addEventListener("click", () => {
    invoke<string>("agent_file_save", { machineId: s.machine_id, path: t.path })
      .then(to => { note(`Saved to Downloads as ${baseName(to)}`); invoke("reveal_file", { path: to }).catch(() => {}); })
      .catch(e => note(readError(e)));
  });
}

/// A picture that fits its box, or shows at its own size; a pinch zooms
/// about the pointer, a two-finger swipe or drag pans.
function imageView(t: MediaTab) {
  const el = h(`<div class="agpview"><div class="agpstage img"><img alt="${esc(t.name)}" draggable="false"></div>
    <div class="agpacts"><button class="dbtn on" data-fit="1">Fit</button><button class="dbtn" data-fit="0">Actual size</button><span class="zoom"></span><span class="meta"></span><span class="sp"></span><span class="hint">Pinch or Ctrl + scroll to zoom, swipe with two fingers or drag to pan</span>${t.path ? fileButtons(t, "Open in Preview") : '<button class="dbtn" data-full="1" title="Full size, to save or copy">Open full size</button>'}</div></div>`);
  const stage = el.querySelector<HTMLElement>(".agpstage")!, img = el.querySelector("img")!;
  const zoomText = el.querySelector(".zoom")!, fitBtn = el.querySelector('[data-fit="1"]')!, actualBtn = el.querySelector('[data-fit="0"]')!;
  // One write per frame, on the GPU layer only: a drag or a wheel sends far
  // more events than frames.
  let frame = 0;
  const apply = () => {
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      const im = t.image;
      if (!im) return;
      img.style.transform = `translate3d(${im.view.x}px, ${im.view.y}px, 0) scale(${im.view.scale})`;
      const pct = `${Math.round(im.view.scale * 100)}%`;
      if (zoomText.textContent !== pct) zoomText.textContent = pct;
      fitBtn.classList.toggle("on", im.view.fit);
      actualBtn.classList.toggle("on", !im.view.fit && im.view.scale === 1);
    });
  };
  const fit = () => { if (t.image) { t.image.view = fitView(t.image.w, t.image.h, stage.clientWidth, stage.clientHeight); apply(); } };
  const actual = () => { if (t.image) { t.image.view = actualView(t.image.w, t.image.h, stage.clientWidth, stage.clientHeight); apply(); } };
  img.onload = () => {
    t.image = { view: { scale: 1, x: 0, y: 0, fit: true }, w: img.naturalWidth, h: img.naturalHeight };
    el.querySelector(".meta")!.textContent = `${img.naturalWidth}×${img.naturalHeight}${t.size ? ` · ${fileSize(t.size)}` : ""}`;
    fit();
  };
  img.src = t.url!;
  new ResizeObserver(() => { if (t.image?.view.fit) fit(); }).observe(stage);
  // The picture holds its own GPU layer only while it moves.
  let still: ReturnType<typeof setTimeout> | undefined;
  const zoom = (factor: number, cx: number, cy: number) => {
    if (!t.image) return;
    stage.classList.add("zooming");
    clearTimeout(still);
    still = setTimeout(() => stage.classList.remove("zooming"), 300);
    const r = stage.getBoundingClientRect();
    t.image.view = zoomAt(t.image.view, factor, cx - r.left, cy - r.top);
    apply();
  };
  stage.addEventListener("wheel", e => {
    e.preventDefault();
    if (!t.image) return;

    // Trackpad pinches send Control-wheel events; ordinary swipes pan.
    if (e.ctrlKey) {
      zoom(Math.exp(-e.deltaY * 0.01), e.clientX, e.clientY);
    } else {
      const dx = e.deltaX * (e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? stage.clientWidth : 1);
      const dy = e.deltaY * (e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? stage.clientHeight : 1);
      t.image.view = { ...t.image.view, x: t.image.view.x - dx, y: t.image.view.y - dy, fit: false };
      apply();
    }
  }, { passive: false });
  // WebKit's own pinch.
  let pinch = 1;
  stage.addEventListener("gesturestart", e => { e.preventDefault(); pinch = 1; });
  stage.addEventListener("gesturechange", e => {
    e.preventDefault();
    const g = e as Event & { scale: number; clientX: number; clientY: number };
    zoom(g.scale / pinch, g.clientX, g.clientY);
    pinch = g.scale;
  });
  stage.addEventListener("pointerdown", e => {
    if (e.button !== 0 || !t.image) return;
    stage.setPointerCapture(e.pointerId);
    stage.classList.add("drag");
    let lx = e.clientX, ly = e.clientY;
    const move = (ev: PointerEvent) => {
      if (!t.image) return;
      t.image.view = { ...t.image.view, x: t.image.view.x + ev.clientX - lx, y: t.image.view.y + ev.clientY - ly, fit: false };
      lx = ev.clientX;
      ly = ev.clientY;
      apply();
    };
    const up = () => {
      stage.classList.remove("drag");
      stage.removeEventListener("pointermove", move);
      stage.removeEventListener("pointerup", up);
    };
    stage.addEventListener("pointermove", move);
    stage.addEventListener("pointerup", up);
  });
  stage.addEventListener("dblclick", () => { if (t.image?.view.fit) actual(); else fit(); });
  el.querySelector('[data-fit="1"]')!.addEventListener("click", fit);
  el.querySelector('[data-fit="0"]')!.addEventListener("click", actual);
  el.querySelector("[data-full]")?.addEventListener("click", () =>
    openViewer([{ url: t.url!, name: t.name, caption: "", saves: t.url!.startsWith("data:") }], 0));
  wireFileButtons(t, el);
  return el;
}

function videoView(t: MediaTab) {
  const el = h(`<div class="agpview"><div class="agpstage"><video controls playsinline preload="auto"></video></div>
    <div class="agpacts"><span class="meta">${esc(fileSize(t.size))}</span><span class="sp"></span>${fileButtons(t, "Open in QuickTime")}</div></div>`);
  const video = el.querySelector("video")!;
  video.onloadedmetadata = () => {
    const secs = Math.round(video.duration);
    const time = Number.isFinite(secs) ? `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")} · ` : "";
    el.querySelector(".meta")!.textContent = `${time}${video.videoWidth}×${video.videoHeight} · ${fileSize(t.size)}`;
  };
  video.onerror = () => {
    el.querySelector(".agpstage")!.replaceWith(h('<div class="agpmsg">This video does not play here. Open it in QuickTime instead.</div>'));
  };
  // WebKit drops a blob video's picture once it ends, so play again shows
  // black: load it afresh, back at its first frame.
  video.onended = () => video.load();
  video.src = t.url!;
  wireFileButtons(t, el);
  return el;
}

function pdfView(t: MediaTab) {
  const el = h(`<div class="agpview"><div class="agpstage pdf"><iframe title="${esc(t.name)}"></iframe></div>
    <div class="agpacts"><span class="meta">PDF · ${esc(fileSize(t.size))}</span><span class="sp"></span>${fileButtons(t, "Open in Preview")}</div></div>`);
  el.querySelector("iframe")!.src = t.url!;
  wireFileButtons(t, el);
  return el;
}

function modelView(t: MediaTab) {
  const el = h(`<div class="agpview"><div class="agpstage model"><div class="agpmsg"><span class="dspin"></span>Drawing ${esc(t.name)}…</div></div>
    <div class="agpacts anim" hidden></div>
    <div class="agpacts"><button class="dbtn" data-reset="1" disabled>Reset view</button><button class="dbtn" data-wire="1" disabled>Wireframe</button><span class="meta"></span><span class="sp"></span><span class="hint">Drag to orbit, right-drag to pan, wheel to zoom</span>${fileButtons(t, "Open")}</div></div>`);
  const stage = el.querySelector<HTMLElement>(".agpstage")!;
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
      if (!P.tabs.includes(t)) return view.dispose();
      t.model = view;
      stage.querySelector(".agpmsg")?.remove();
      const i = view.info;
      const clips = i.clips ? ` · ${i.clips} clip${i.clips > 1 ? "s" : ""}` : "";
      el.querySelector(".meta")!.textContent = `${i.format} · ${i.triangles.toLocaleString()} triangles${i.meshes > 1 ? ` · ${i.meshes} meshes` : ""}${clips} · ${fileSize(t.size)}`;
      el.querySelectorAll<HTMLButtonElement>("[data-reset], [data-wire]").forEach(b => { b.disabled = false; });
      if (view.anim) {
        const controls = el.querySelector<HTMLElement>(".anim");
        if (controls) { setSessionPortal(controls, createElement(ModelControls, { animation: view.anim })); controls.hidden = false; }
      }
      view.resize();
    })
    .catch(e => {
      stage.innerHTML = `<div class="agpmsg derr">${esc(`Could not draw ${t.name}: ${e instanceof Error ? e.message : String(e)}`)}</div>`;
    });
  el.querySelector("[data-reset]")!.addEventListener("click", () => t.model?.reset());
  el.querySelector("[data-wire]")!.addEventListener("click", e => {
    t.wire = !t.wire;
    t.model?.wireframe(t.wire);
    (e.currentTarget as HTMLElement).classList.toggle("on", t.wire);
  });
  wireFileButtons(t, el);
  return el;
}
