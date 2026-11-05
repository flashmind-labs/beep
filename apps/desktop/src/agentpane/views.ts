// What each kind of tab shows: text, a terminal, a picture, a video, a PDF
// or a 3D model.
import { actualView, baseName, clipTime, fitView, resolvePath, zoomAt } from "./media-state.ts";
import { highlight, languageOf, MAX_LINES, shortSize } from "./files-state.ts";
import type { Animation } from "./model3d.ts";
import { fileSize, openViewer } from "../chat/media.ts";
import { attach, killTerm, type PaneTerm } from "./terminal.ts";
import { addMention } from "../agents/workdrop.ts";
import { type TreeText, type MediaTab, P, closeTab, saveWorkFile } from "./media.ts";
import { readFile, readError, drawTermStatus } from "./read.ts";
import { esc } from "../ui/format.ts";
import { h } from "../ui/dom.ts";
import { invoke } from "../ui/tauri.ts";

export function viewOf(t: MediaTab): HTMLElement {
  if (t.error) return h(`<div class="agpview"><div class="agpmsg derr">${esc(t.error)}</div></div>`);
  if (t.kind === "text") return t.text ? textView(t, t.text) : h(`<div class="agpview"><div class="agpmsg"><span class="dspin"></span>Reading ${esc(t.name)} from ${esc(P.session?.machine_name ?? "the machine")}…</div></div>`);
  if (t.kind === "term") return t.term ? termView(t, t.term) : h(`<div class="agpview"><div class="agpmsg"><span class="dspin"></span>Opening a terminal on ${esc(P.session?.machine_name ?? "the machine")}…</div></div>`);
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
    body = `<div class="agpmsg">${esc(t.name)} is not text (${esc(shortSize(text.size))}). Save it to open it on this Mac.</div>`;
  } else {
    const all = text.text.split("\n");
    if (all.length > 1 && all.at(-1) === "") all.pop();
    lines = all.length;
    const shown = all.slice(0, MAX_LINES);
    const width = String(shown.length).length;
    body = `<div class="agcode" style="--lnw:${width}ch"><pre>${shown.map((l, i) => `<span class="ln">${i + 1}</span>${highlight(l, lang)}`).join("\n")}</pre></div>`;
  }
  const cut = text.cut || lines > MAX_LINES ? ` · the first ${lines > MAX_LINES ? `${MAX_LINES.toLocaleString()} lines` : shortSize(text.text.length)} shown` : "";
  const about = [rel, lang, text.binary ? "" : `${lines.toLocaleString()} lines`, shortSize(text.size)].filter(Boolean).join(" · ") + cut;
  const el = h(`<div class="agpview">${body}<div class="agpacts"><span class="meta mono" title="${esc(rel)}">${esc(about)}</span><span class="sp"></span><button class="dbtn" data-tment="1">Mention</button><button class="dbtn" data-tsave="1">Save</button></div></div>`);
  const meta = el.querySelector(".meta")!;
  el.querySelector("[data-tment]")!.addEventListener("click", () => addMention(rel));
  el.querySelector("[data-tsave]")!.addEventListener("click", () => saveWorkFile(rel, note => { meta.textContent = note; }));
  return el;
}

/// A terminal tab: its live screen, and a footer to attach again or stop it.
function termView(t: MediaTab, term: PaneTerm) {
  const el = h(`<div class="agpview term"><div class="agpacts"><span class="meta mono"></span><span class="sp"></span><button class="dbtn" data-tattach="1" hidden>Attach</button><button class="dbtn warn" data-tkill="1">Close</button></div></div>`);
  el.prepend(term.screen.el);
  el.querySelector("[data-tattach]")!.addEventListener("click", () => { term.tries = 0; attach(term); });
  el.querySelector("[data-tkill]")!.addEventListener("click", () => {
    if (term.row.state === "running" && term.link !== "ended" && !confirm(`Stop ${t.name} on ${P.session?.machine_name ?? "the machine"}?`)) return;
    killTerm(term).then(() => { t.term = undefined; closeTab(t.id); }).catch(e => { el.querySelector(".meta")!.textContent = readError(e); });
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

/// A picture that fits its box, or shows at its own size; the wheel or a
/// pinch zooms about the pointer, a drag pans.
function imageView(t: MediaTab) {
  const el = h(`<div class="agpview"><div class="agpstage img"><img alt="${esc(t.name)}" draggable="false"></div>
    <div class="agpacts"><button class="dbtn on" data-fit="1">Fit</button><button class="dbtn" data-fit="0">Actual size</button><span class="zoom"></span><span class="meta"></span><span class="sp"></span><span class="hint">Wheel or pinch to zoom, drag to pan</span>${t.path ? fileButtons(t, "Open in Preview") : '<button class="dbtn" data-full="1" title="Full size, to save or copy">Open full size</button>'}</div></div>`);
  const stage = el.querySelector<HTMLElement>(".agpstage")!, img = el.querySelector("img")!;
  const apply = () => {
    const im = t.image;
    if (!im) return;
    img.style.transform = `translate(${im.view.x}px, ${im.view.y}px) scale(${im.view.scale})`;
    el.querySelector(".zoom")!.textContent = `${Math.round(im.view.scale * 100)}%`;
    el.querySelector('[data-fit="1"]')!.classList.toggle("on", im.view.fit);
    el.querySelector('[data-fit="0"]')!.classList.toggle("on", !im.view.fit && im.view.scale === 1);
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
  const zoom = (factor: number, cx: number, cy: number) => {
    if (!t.image) return;
    const r = stage.getBoundingClientRect();
    t.image.view = zoomAt(t.image.view, factor, cx - r.left, cy - r.top);
    apply();
  };
  stage.addEventListener("wheel", e => {
    e.preventDefault();
    // A pinch comes as the wheel with Control held, in small steps.
    zoom(Math.exp(-e.deltaY * (e.ctrlKey ? 0.01 : 0.002)), e.clientX, e.clientY);
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
  const el = h(`<div class="agpview"><div class="agpstage"><video controls playsinline preload="metadata"></video></div>
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
    <div class="agpacts anim" hidden><button class="dbtn" data-play="1"></button><select aria-label="Clip"></select><input type="range" min="0" step="any" aria-label="Time"><span class="time"></span><button class="dbtn" data-loop="1">Loop</button></div>
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
      if (view.anim) animControls(el.querySelector(".anim")!, view.anim);
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

/// A model's animation row: play or pause, the clip, a scrub bar with
/// the time, and loop. It plays on opening unless motion is reduced.
function animControls(bar: HTMLElement, a: Animation) {
  const play = bar.querySelector<HTMLButtonElement>("[data-play]")!, loop = bar.querySelector<HTMLButtonElement>("[data-loop]")!;
  const pick = bar.querySelector("select")!, scrub = bar.querySelector("input")!, time = bar.querySelector(".time")!;
  pick.innerHTML = a.clips.map((c, i) => `<option value="${i}">${esc(c.name)}</option>`).join("");
  pick.hidden = a.clips.length < 2;
  a.onchange = () => {
    const end = a.clips[a.clip].duration, now = a.time();
    // Only on a change: each frame calls this, and a button whose text node is
    // replaced between press and release gets no click in WebKit.
    const label = a.playing ? "Pause" : "Play";
    if (play.textContent !== label) play.textContent = label;
    loop.classList.toggle("on", a.loop);
    scrub.max = String(end);
    scrub.value = String(now);
    time.textContent = `${clipTime(now)} / ${clipTime(end)}`;
  };
  play.addEventListener("click", () => a.play(!a.playing));
  loop.addEventListener("click", () => a.setLoop(!a.loop));
  pick.addEventListener("change", () => a.select(Number(pick.value)));
  // Scrubbing pauses it, so the bar does not pull away from the pointer.
  scrub.addEventListener("input", () => {
    a.play(false);
    a.seek(Number(scrub.value));
  });
  bar.hidden = false;
  a.onchange();
  if (!matchMedia("(prefers-reduced-motion: reduce)").matches) a.play(true);
}
