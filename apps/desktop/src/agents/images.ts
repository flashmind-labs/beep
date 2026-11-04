// Images for a coding agent: attached to a message, and shown in its log.
import type { Attached } from "./agentqueue.ts";
import { fileBase64 } from "../chat/attach.ts";
import { $, esc, h, ICON, invoke, S } from "../bar/core.ts";
import { GONE } from "../chat/media.ts";
import { mentionChips, wireMentionChips } from "./workdrop.ts";
import { type OpenAgent, AG, AGENT_IMAGE_TYPES, AGENT_MAX_IMAGES, AGENT_MAX_IMAGE, AGENT_MAX_IMAGES_TOTAL, agentPics, AGENT_PICS_KEPT, agentImages, agentThumbs } from "./agents.ts";
import { noAgentImages, redrawAgentHints } from "./session.ts";

let pastedCount = 0;

/// A name for an image, unique among those waiting; a pasted one is
/// named "Pasted image 1.png" and so on rather than "image.png".
export function agentImageName(f: File) {
  const ext = ({ "image/png": "png", "image/jpeg": "jpg", "image/gif": "gif", "image/webp": "webp" } as Record<string, string>)[f.type] || "png";
  const base = f.name && f.name !== `image.${ext}` && f.name !== "image.png" ? f.name : `Pasted image ${++pastedCount}.${ext}`;
  const dot = base.lastIndexOf(".");
  let name = base;
  for (let n = 2; AG.att.some(a => a.name === name); n++) name = dot > 0 ? `${base.slice(0, dot)} ${n}${base.slice(dot)}` : `${base} ${n}`;
  return name;
}

export function agentAttachError(text: string) {
  AG.attError = text;
  drawAgentAttached();
  clearTimeout(AG.attTimer);
  AG.attTimer = setTimeout(() => { AG.attError = ""; drawAgentAttached(); }, 5000);
}

/// Adds pasted or dropped images to the session's next message, refusing
/// what the agent cannot take rather than shrinking or dropping it.
export async function agentAttach(files: Iterable<File>) {
  if (noAgentImages()) return agentAttachError("This Beep server does not take images yet.");
  for (const f of [...files]) {
    if (!AGENT_IMAGE_TYPES.includes(f.type)) { agentAttachError(`${f.name || "That file"} is not a PNG, JPEG, GIF or WebP image`); continue; }
    if (AG.att.length >= AGENT_MAX_IMAGES) { agentAttachError(`Up to ${AGENT_MAX_IMAGES} images in one message`); break; }
    if (f.size > AGENT_MAX_IMAGE) { agentAttachError(`${f.name || "That image"} is over ${AGENT_MAX_IMAGE >> 20} MB`); continue; }
    const used = AG.att.reduce((n, a) => n + a.size, 0);
    if (used + f.size > AGENT_MAX_IMAGES_TOTAL) { agentAttachError(`Images up to ${AGENT_MAX_IMAGES_TOTAL >> 20} MB in all in one message`); continue; }
    try {
      const data = await fileBase64(f);
      AG.att.push({ name: agentImageName(f), mime: f.type, size: f.size, data, url: `data:${f.type};base64,${data}` });
    } catch {
      agentAttachError(`${f.name || "That image"} could not be read`);
    }
  }
  drawAgentAttached();
}

/// The chips of waiting images above the field, as in the mock.
export function drawAgentAttached() {
  const field = $("#agfield");
  if (!field) return;
  const old = $("#agatt");
  // Paths dragged in from the Files tab and files uploaded, after the images.
  const mentions = mentionChips();
  const el = AG.att.length || AG.attError || mentions ? h(`<div class="ag-att" id="agatt">${AG.att.map((a, i) =>
    `<span class="att"><img src="${esc(a.url)}" alt=""><span class="nm">${esc(a.name)}</span><button aria-label="Remove ${esc(a.name)}" data-agatt="${i}">${ICON.x}</button></span>`).join("")}${mentions}${AG.attError ? `<span class="derr">${esc(AG.attError)}</span>` : ""}</div>`) : null;
  el?.querySelectorAll<HTMLElement>("[data-agatt]").forEach(b => b.addEventListener("click", () => {
    AG.att.splice(Number(b.dataset.agatt), 1);
    drawAgentAttached();
    $("#agmsg")?.focus();
  }));
  if (el) wireMentionChips(el);
  if (old) el ? old.replaceWith(el) : old.remove();
  else if (el) field.before(el);
  if (S.view === "agent") redrawAgentHints();
}

/// The picture `key` of session `o` as `command` gives it, read from its
/// machine the first time and kept in `cache`. Only one the machine no longer
/// has stays gone; anything else tries again.
function loadOnce(cache: typeof agentImages, command: string, o: OpenAgent, key: string) {
  let got = cache.get(key);
  if (got === undefined) {
    got = invoke<string>(command, { machineId: o.machine_id, session: o.session, key }).then(url => {
      cache.set(key, url);
      return url;
    }, e => {
      if (String(e) === GONE) cache.set(key, null);
      else cache.delete(key);
      return null;
    });
    cache.set(key, got);
  }
  return Promise.resolve(got);
}

export const loadAgentImage = (o: OpenAgent, key: string) => loadOnce(agentImages, "agent_image", o, key);

/// The picture a tile of the log stands for, whole: what opens, saves or
/// copies, as the tile itself may show a thumbnail. Null when it cannot be read.
export const fullPic = (o: OpenAgent, img: HTMLImageElement) => img.dataset.key ? loadAgentImage(o, img.dataset.key) : Promise.resolve(img.src);

/// Fills the session pictures drawn before their bytes were here.
export function fillAgentPics(b: HTMLElement) {
  const o = AG.open;
  if (!o) return;
  for (const img of b.querySelectorAll<HTMLImageElement>("img.pic[data-key]:not([src])")) {
    loadOnce(agentThumbs, "agent_thumb", o, img.dataset.key!).then(url => {
      if (url) img.src = url;
      else if (img.isConnected && agentThumbs.get(img.dataset.key!) === null) img.replaceWith(h(gonePic(img.alt)));
    });
  }
}

/// A session picture that is not here: its icon and name.
export const gonePic = (label: string) => `<span class="pic none" title="${esc(label)}">${ICON.image}${esc(label)}</span>`;

/// The images a message sent from here carried, under the keys the log
/// will name them by, so they show without asking the machine.
export function keepSentImages(keys: string[], images: { mime: string; data: string }[]) {
  keys.forEach((key, i) => { if (images[i]) agentImages.set(key, `data:${images[i].mime};base64,${images[i].data}`); });
}

/// Keeps the thumbnails of images just sent, for the transcript.
export function keepAgentPics(o: OpenAgent, images: Attached[]) {
  for (const a of images) {
    const key = `${o.session}\n${a.name}`;
    agentPics.delete(key);
    agentPics.set(key, a.url);
  }
  while (agentPics.size > AGENT_PICS_KEPT) agentPics.delete(agentPics.keys().next().value!);
}
