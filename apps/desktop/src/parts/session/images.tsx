// Images for a coding agent: attached to a message, and shown in its log.
import type { Attached } from "../../stores/agents/agentqueue.ts";
import { AGENT_IMAGE_BYTES, AGENT_IMAGES_BYTES, fileBase64, picPreview } from "../../stores/chat/pics.ts";
import { GONE } from "./media.tsx";
import { MN } from "../../stores/agents/mentions.ts";
import { type OpenAgent, AG, AGENT_MAX_IMAGES, agentPics, AGENT_PICS_KEPT, agentImages, agentThumbs } from "../../stores/agents/agents-state.ts";
import { gonePic, retryIn } from "./pic-state.ts";
import { invoke } from "../../ui/tauri.ts";
import { fillInlineImages } from "./inline-images.ts";
import { readFile } from "../../stores/agentpane/read-file.ts";
import { bytesOf } from "../../stores/agentpane/media-state.ts";
import type { Preview } from "./composer-ui.tsx";

let pastedCount = 0;

/// A name for an image, unique among those waiting in `list`; a pasted one
/// is named "Pasted image 1.png" and so on rather than "image.png".
function agentImageName(f: File, list: Attached[]) {
  const ext = ({ "image/png": "png", "image/jpeg": "jpg", "image/gif": "gif", "image/webp": "webp" } as Record<string, string>)[f.type] || "png";
  const base = f.name && f.name !== `image.${ext}` && f.name !== "image.png" ? f.name : `Pasted image ${++pastedCount}.${ext}`;
  const dot = base.lastIndexOf(".");
  let name = base;
  for (let n = 2; list.some(a => a.name === name); n++) name = dot > 0 ? `${base.slice(0, dot)} ${n}${base.slice(dot)}` : `${base} ${n}`;
  return name;
}

/// Whether `file` goes to the agent as the image it is, byte for byte: a
/// supported type small enough that the combined budget never shrinks it.
/// Its upload would only send the same bytes a second time.
export const staysInline = (file: File) =>
  file.size <= Math.floor(AGENT_IMAGES_BYTES / AGENT_MAX_IMAGES) && ["image/png", "image/jpeg", "image/gif", "image/webp"].includes(file.type);

const attachmentWork = new WeakMap<Attached[], Promise<unknown>>();

/// Preserve readable images within the prompt limits. Serialize batches
/// sharing a composer so their combined bytes stay within the budget. Gives
/// the image attached, or null when there was no room or it did not read.
export function attachPreview(list: Attached[], file: File, original: string | File | null) {
  const work = (attachmentWork.get(list) ?? Promise.resolve()).then(() => prepareAttachment(list, file, original));
  attachmentWork.set(list, work);
  return work;
}

async function prepareAttachment(list: Attached[], file: File, original: string | File | null) {
  if (list.length >= AGENT_MAX_IMAGES) return null;

  try {
    const preview = await picPreview(file);
    const data = await fileBase64(preview);
    const image = { name: agentImageName(preview, list), mime: preview.type, size: preview.size, data, url: `data:${preview.type};base64,${data}`, original };
    const before = [...list];
    const fitted = await fitAttachedImages([...before, image]);
    const replacements = new Map(before.map((image, i) => [image, fitted[i]]));
    if (list.length >= AGENT_MAX_IMAGES) return null;

    for (let i = 0; i < list.length; i++) list[i] = replacements.get(list[i]) ?? list[i];
    const added = fitted[before.length];
    if (added) list.push(added);
    return added ?? null;
  } catch {
    // An uploaded file is still mentioned.
    return null;
  }
}

/// A send can combine completed attachments with a batch still uploading.
/// Apply the same limits after combining them, keeping every file reference.
export async function fitAttachedImages(images: Attached[]): Promise<Attached[]> {
  const kept = images.slice(0, AGENT_MAX_IMAGES);
  if (kept.every(image => image.size <= AGENT_IMAGE_BYTES) && kept.reduce((n, image) => n + image.size, 0) <= AGENT_IMAGES_BYTES) return kept;
  const budget = Math.min(AGENT_IMAGE_BYTES, Math.floor(AGENT_IMAGES_BYTES / kept.length));

  return Promise.all(kept.map(async image => {
    if (image.size <= budget) return image;
    const smaller = await picPreview(new File([bytesOf(image.data)], image.name, { type: image.mime }), budget);
    const data = await fileBase64(smaller);
    return { ...image, mime: smaller.type, size: smaller.size, data, url: `data:${smaller.type};base64,${data}` };
  }));
}

/// The previews of `list` for its chips. Opening one shows the picture the
/// user attached, including when the inline image was resized.
export function previewsOf(list: Attached[]): Preview[] {
  return list.map(image => ({ url: image.url, name: image.name, full: () => fullOf(image) }));
}

async function fullOf({ original, url }: Attached) {
  if (original instanceof File) return `data:${original.type};base64,${await fileBase64(original)}`;
  const file = typeof original === "string" ? MN.files.get(original) : undefined;

  return file ? `data:${file.mime};base64,${file.data}` : url;
}

/// The thumbnails that failed for a reason other than being gone: failures
/// in a row, and when to ask again. Not asked again on every poll: each ask
/// crosses the network, and asking all the time starves the session's reads.
const failedThumbs = new Map<string, { fails: number; until: number }>();

/// The picture `key` of session `o` as `command` gives it, read from its
/// machine the first time and kept in `cache`. Only one the machine no longer
/// has stays gone; anything else tries again.
function loadOnce(cache: typeof agentImages, command: string, o: OpenAgent, key: string) {
  let got = cache.get(key);

  if (got === undefined) {
    got = invoke<string>(command, { machineId: o.machine_id, session: o.session, key }).then(url => {
      cache.set(key, url);
      if (cache === agentThumbs) failedThumbs.delete(key);
      return url;
    }, e => {
      if (String(e) === GONE) cache.set(key, null);
      else cache.delete(key);

      if (cache === agentThumbs && String(e) !== GONE) {
        const fails = (failedThumbs.get(key)?.fails ?? 0) + 1;
        failedThumbs.set(key, { fails, until: Date.now() + retryIn(fails) });
      }

      return null;
    });
    cache.set(key, got);
  }

  return Promise.resolve(got);
}

const loadAgentImage = (o: OpenAgent, key: string) => loadOnce(agentImages, "agent_image", o, key);

/// The picture a tile of the log stands for, whole: what opens, saves or
/// copies, as the tile itself may show a thumbnail. Null when it cannot be read.
export async function fullPic(o: OpenAgent, img: HTMLImageElement) {
  if (img.dataset.original) {
    const { bytes, mime } = await readFile(o.machine_id, img.dataset.original, o.session, () => {});
    if (!mime.startsWith("image/")) throw new Error("The file is not an image");
    return `data:${mime};base64,${await fileBase64(new Blob([bytes], { type: mime }))}`;
  }

  return img.dataset.key ? loadAgentImage(o, img.dataset.key) : img.src;
}

/// A picture of `key` already here: whole, or its thumbnail.
function picHere(key: string) {
  const whole = agentImages.get(key), thumb = agentThumbs.get(key);
  return typeof whole === "string" ? whole : typeof thumb === "string" ? thumb : null;
}

let refill: ReturnType<typeof setTimeout> | undefined;

/// Fills the session pictures drawn without their bytes (all those read by
/// key): from what is here at once, else read from the machine. One that
/// failed is asked for again once its wait is over, not on every draw.
export function fillAgentPics(b: HTMLElement) {
  const o = AG.open;
  if (!o) return;
  const session = AG.page?.session;
  if (session) fillInlineImages(b, session, () => AG.open === o && AG.page?.session.cwd === session.cwd);
  let wait = Infinity;

  for (const img of b.querySelectorAll<HTMLImageElement>("img.pic[data-key]:not([src])")) {
    const key = img.dataset.key!, here = picHere(key);
    if (here) { img.src = here; continue; }
    const until = failedThumbs.get(key)?.until ?? 0;
    if (until > Date.now()) { wait = Math.min(wait, until - Date.now()); continue; }
    loadOnce(agentThumbs, "agent_thumb", o, key).then(url => {
      if (!b.isConnected || AG.open !== o) return;

      // The row may have been drawn again meanwhile: fill the tiles there now.
      for (const tile of b.querySelectorAll<HTMLImageElement>(`img.pic[data-key="${key}"]:not([src])`)) {
        if (url) tile.src = url;
        else if (agentThumbs.get(key) === null) tile.outerHTML = gonePic(tile.alt);
      }

      const failed = failedThumbs.get(key);
      if (!url && failed) fillLater(b, failed.until - Date.now());
    });
  }

  if (wait < Infinity) fillLater(b, wait);
}

/// Fills the pictures again in `ms`, for those that failed.
function fillLater(b: HTMLElement, ms: number) {
  clearTimeout(refill);
  refill = setTimeout(() => { if (b.isConnected && AG.open) fillAgentPics(b); }, Math.max(0, ms));
}

/// The images a message sent from here carried, under the keys the log
/// will name them by, so they show without asking the machine.
export function keepSentImages(o: OpenAgent, names: readonly string[], keys: readonly string[]) {
  keys.forEach((key, i) => {
    const url = agentPics.get(`${o.session}\n${names[i]}`);
    if (url) agentImages.set(key, url);
  });
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
