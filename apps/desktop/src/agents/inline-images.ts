// Markdown images whose files belong to the session's machine.
import { baseName, resolvePath } from "../agentpane/media-state.ts";
import type { FilePart } from "../agentpane/pane-state.ts";
import { invoke } from "../ui/tauri.ts";
import { gonePic, retryIn } from "./pic-state.ts";

/// Remove local sources before the webview tries to load them. Keep the
/// sanitized attributes escaped exactly as the Markdown renderer supplied them.
export function prepareInlineImages(html: string) {
  return html.replace(/<img\b[^>]*>/gi, tag => {
    const attribute = /\s([^\s=/>]+)\s*=\s*("[^"]*"|'[^']*')/g;
    const attrs = [...tag.matchAll(attribute)];
    const src = attrs.find(a => a[1].toLowerCase() === "src");
    if (!src || attrs.some(a => a[1].toLowerCase() === "data-key")) return tag;
    const path = src[2].slice(1, -1);
    if (!path || /^(?:\/\/|#|[a-z][a-z\d+.-]*:)/i.test(path) && !/^file:/i.test(path)) return tag;
    tag = tag.replace(attribute, (attr, name: string, value: string) => {
      if (name.toLowerCase() === "src") return ` data-file=${value}`;
      if (name.toLowerCase() === "class") return ` class=${value[0]}pic ${value.slice(1)}`;
      return attr;
    });
    return attrs.some(a => a[1].toLowerCase() === "class") ? tag : tag.replace(/<img\b/i, '<img class="pic"');
  });
}

const MAX_IMAGE = 16 * 1024 * 1024;
const MAX_CACHE = 32 * 1024 * 1024;
type Cached = { url?: string | null; fails: number; until: number; missing: boolean };
const cache = new Map<string, Cached>();
const pending = new Set<string>();
let cachedBytes = 0;
let retry: ReturnType<typeof setTimeout> | undefined;
let refill = () => {};

function keep(key: string, value: Cached) {
  cachedBytes -= cache.get(key)?.url?.length ?? 0;
  cache.delete(key);
  cache.set(key, value);
  cachedBytes += value.url?.length ?? 0;

  while (cache.size > 64 || cachedBytes > MAX_CACHE) {
    const first = cache.keys().next().value!;
    cachedBytes -= cache.get(first)?.url?.length ?? 0;
    cache.delete(first);
  }
}

function imagePath(cwd: string, src: string) {
  if (/^file:/i.test(src)) {
    const url = new URL(src);
    if (url.hostname && url.hostname !== "localhost") throw new Error("Remote file URL");
    src = url.pathname;
  } else src = src.split(/[?#]/, 1)[0];

  return resolvePath(cwd, decodeURIComponent(src));
}

async function readImage(machineId: string, path: string, session: string, current: () => boolean) {
  const parts: Uint8Array<ArrayBuffer>[] = [];
  let offset = 0, size = -1, mime = "";

  do {
    if (!current()) return;
    const part = await invoke<FilePart>("agent_file", { machineId, path, offset, session });
    if (!current()) return;
    if (!Number.isSafeInteger(part.size) || part.size <= 0 || part.size > MAX_IMAGE || !part.mime.startsWith("image/")) return null;
    if (size >= 0 && (size !== part.size || mime !== part.mime)) throw new Error("Image changed while reading");
    size = part.size;
    mime = part.mime;
    const data = atob(part.data);
    if (!data.length || offset + data.length > size) throw new Error("Incomplete image");
    parts.push(Uint8Array.from(data, c => c.charCodeAt(0)));
    offset += data.length;
  } while (offset < size);

  return new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result as string);
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(new Blob(parts, { type: mime }));
  });
}

/// Only the current view may draw results or start another chunk. Concurrent
/// reads and retained bytes are bounded across redraws and session switches.
export function fillInlineImages(root: HTMLElement, session: { machine_id: string; id: string; cwd: string }, current: () => boolean) {
  clearTimeout(retry);
  refill = () => { if (root.isConnected && current()) fillInlineImages(root, session, current); };
  let wait = Infinity;

  for (const img of root.querySelectorAll<HTMLImageElement>("img.pic[data-file]:not([src])")) {
    let path: string;

    try { path = imagePath(session.cwd, img.dataset.file!); }
    catch { img.outerHTML = gonePic(img.alt || "Image"); continue; }

    const key = JSON.stringify([session.machine_id, path]);
    const got = cache.get(key);
    if (got?.url) { keep(key, got); img.src = got.url; continue; }
    if (got?.url === null) {
      const label = img.alt || baseName(path);
      const message = got.missing ? "The file is no longer on disk. Unable to load image." : label;
      img.outerHTML = gonePic(label, message);
      continue;
    }
    if (got && got.until > Date.now()) { wait = Math.min(wait, got.until - Date.now()); continue; }
    if (pending.has(key) || pending.size >= 3) continue;
    pending.add(key);
    const active = () => root.isConnected && current();
    void readImage(session.machine_id, path, session.id, active).then(url => {
      if (url !== undefined) keep(key, { url, missing: false, fails: 0, until: 0 });
    }, e => {
      if (!active()) return;
      const fails = (got?.fails ?? 0) + 1;
      const gone = /no such file|os error 2|file not found|no longer available/i.test(String(e));
      keep(key, { url: gone ? null : undefined, missing: gone, fails, until: Date.now() + retryIn(fails) });
    }).finally(() => { pending.delete(key); refill(); });
  }

  if (wait < Infinity) retry = setTimeout(() => refill(), wait);
}
