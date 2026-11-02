import { agentAttach } from "./agents.ts";
import { focusAsk } from "./chat.ts";
import { $, bar, esc, h, ICON, invoke, S } from "./core.ts";
import { refreshHints } from "./hints.ts";
import { VOICE_ICON } from "./live.ts";
import { fileKind, fileSize, GONE } from "./media.ts";

// ---------- Attached images and files ----------

/// A file waiting in the prompt, its bytes in base64 (`FileUpload`, and
/// its size).
export interface Attachment {
  name: string;
  mime: string;
  size: number;
  data: string;
}

/// A file a prompt carried: one just sent, or one the server keeps
/// (`FileRef`), which has an id to open it by.
export interface AskFile {
  name: string;
  mime: string;
  size: number;
  id?: string;
}

// Pictures and files pasted or dropped into the prompt, sent with the next
// message. Each picture is shrunk and sent as JPEG, as the phone does; other
// files go as they are, up to MAX_UPLOAD in all.
export const attached: { pics: string[]; files: Attachment[]; error: string; timer?: ReturnType<typeof setTimeout> } = { pics: [], files: [], error: "" };
export const MAX_IMAGES = 8;
export const MAX_UPLOAD = 5 * 1024 * 1024;
export const PIC_SIDE = 1024;

export const attachedCount = () => attached.pics.length + attached.files.length;

/// An attached file's chip: its name and size, with a remove button while
/// it waits in the prompt, as a button that opens it once it is kept.
export function fileChip(f: AskFile, i?: number) {
  const tag = f.id ? "button" : "span";
  // A spoken message's recording, kept with it.
  if (i === undefined && f.mime?.startsWith("audio/")) {
    const about = ["Audio", fileSize(f.size)].filter(Boolean).join(", ");
    return `<${tag} class="dachip" aria-label="Open your recording"><span class="ic">${VOICE_ICON.mic}</span><span style="min-width:0"><span class="nm">Your recording</span><span class="sz">${esc(about)}</span></span></${tag}>`;
  }
  const about = [fileKind(f), fileSize(f.size)].filter(Boolean).join(", ");
  const remove = i === undefined ? "" : `<button aria-label="Remove ${esc(f.name)}" data-i="${i}">${ICON.x}</button>`;
  return `<${tag} class="dachip"><span class="ic">${ICON.doc}</span><span style="min-width:0"><span class="nm">${esc(f.name)}</span><span class="sz">${esc(about)}</span></span>${remove}</${tag}>`;
}

/// The files a prompt carried, under it. A kept one opens like a file Beep sent.
export function askFiles(files: AskFile[]) {
  const row = h(`<div class="dachips">${files.map(f => fileChip(f)).join("")}</div>`);
  [...row.children].forEach((chip, i) => {
    const file = files[i];
    if (!file.id) return;
    chip.addEventListener("click", async () => {
      if (chip.classList.contains("gone")) return;
      try { await invoke("open_file", { id: file.id, name: file.name }); }
      catch (e) {
        chip.querySelector(".sz")!.textContent = String(e) === GONE ? GONE : String(e);
        if (String(e) === GONE) chip.classList.add("gone");
      }
    });
  });
  return row;
}

/// The strip of attached pictures and files above the prompt, or the error
/// from the last paste, with the placeholder and hints to match.
export function drawAttached() {
  const old = $("#thumbs"), ask = $<HTMLTextAreaElement>("#ask"), input = ask?.closest(".dinput");
  if (!ask || !input) return;
  const pics = attached.pics.length, n = attachedCount();
  const el = n || attached.error ? h(`<div class="dthumbs" id="thumbs">${attached.pics.map((url, i) =>
    `<span class="dthumb"><img src="${esc(url)}" alt="Image ${i + 1}"><button aria-label="Remove image ${i + 1}" data-i="${i}">${ICON.x}</button></span>`).join("")}${attached.files.map((f, i) => fileChip(f, pics + i)).join("")}${attached.error ? `<span class="derr">${esc(attached.error)}</span>` : ""}</div>`) : null;
  el?.querySelectorAll<HTMLElement>("button").forEach(b => b.addEventListener("click", () => { unattach(+b.dataset.i!); focusAsk(); }));
  if (old) el ? old.replaceWith(el) : old.remove();
  else if (el) input.before(el);
  if (n) {
    ask.dataset.ph ??= ask.placeholder;
    const what = attached.files.length ? (n > 1 ? "these" : "this file") : (n > 1 ? "these images" : "this image");
    ask.placeholder = `Ask about ${what}…`;
  } else if (ask.dataset.ph) {
    ask.placeholder = ask.dataset.ph;
    delete ask.dataset.ph;
  }
  refreshHints();
}

/// Removes the `i`th attachment, counting pictures first.
export function unattach(i: number) {
  if (i < attached.pics.length) attached.pics.splice(i, 1);
  else attached.files.splice(i - attached.pics.length, 1);
  attached.error = "";
  drawAttached();
}

export function attachError(text: string) {
  attached.error = text;
  drawAttached();
  clearTimeout(attached.timer);
  attached.timer = setTimeout(() => { attached.error = ""; drawAttached(); }, 4000);
}

/// Adds files from a paste or a drop: pictures shrunk, up to `MAX_IMAGES`;
/// other files, or pictures that cannot be drawn, as they are.
export async function attach(files: Iterable<File>) {
  const all = [...files];
  if (!all.length) return false;
  for (const f of all) {
    if (attachedCount() >= MAX_IMAGES) { attachError(`Up to ${MAX_IMAGES} attachments in one message`); break; }
    if (f.type.startsWith("image/")) {
      try { attached.pics.push(await shrinkPic(f)); continue; } catch { /* sent as a file, such as a HEIC */ }
    }
    const used = attached.files.reduce((n, x) => n + x.size, 0);
    if (used + f.size > MAX_UPLOAD) { attachError(`Files up to ${MAX_UPLOAD >> 20} MB in one message`); continue; }
    try { attached.files.push({ name: f.name || "file", mime: f.type, size: f.size, data: await fileBase64(f) }); }
    catch { attachError(`${f.name || "That file"} could not be read`); }
  }
  drawAttached();
  return true;
}

/// `file`'s bytes in base64. A folder fails to read.
export function fileBase64(file: Blob) {
  return new Promise<string>((done, fail) => {
    const reader = new FileReader();
    reader.onload = () => done(String(reader.result).split(",")[1] ?? "");
    reader.onerror = () => fail(reader.error);
    reader.readAsDataURL(file);
  });
}

/// `file` as a JPEG data URI no longer than `PIC_SIDE` on its longest side.
export async function shrinkPic(file: Blob) {
  const img = await createImageBitmap(file);
  const scale = Math.min(1, PIC_SIDE / Math.max(img.width, img.height, 1));
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(img.width * scale));
  canvas.height = Math.max(1, Math.round(img.height * scale));
  const cx = canvas.getContext("2d")!;
  // JPEG has no transparency; a screenshot's clear parts become white, not black.
  cx.fillStyle = "#fff";
  cx.fillRect(0, 0, canvas.width, canvas.height);
  cx.drawImage(img, 0, 0, canvas.width, canvas.height);
  img.close();
  return canvas.toDataURL("image/jpeg", 0.8);
}

/// Whether the agent view's message field is on screen, to take what is
/// pasted or dropped.
const agentField = () => S.view === "agent" && !!$("#agmsg");

document.addEventListener("paste", e => {
  const agent = agentField();
  if (!agent && (S.view !== "chat" || !$("#ask"))) return;
  const files = [...(e.clipboardData?.items ?? [])].filter(i => i.kind === "file").map(i => i.getAsFile()).filter((f): f is File => !!f);
  if (!files.length) return;
  e.preventDefault();
  agent ? agentAttach(files) : attach(files);
});

// Dropping images and files on the chat attaches them too.
bar.addEventListener("dragover", e => {
  const agent = agentField();
  if ((S.view !== "chat" && !agent) || !e.dataTransfer?.types.includes("Files")) return;
  e.preventDefault();
  if (!$("#ddrop")) bar.append(h(`<div class="ddrop" id="ddrop">${ICON.image}${agent ? "Drop images to send them to the agent" : "Drop images or files to ask about them"}</div>`));
});
bar.addEventListener("dragleave", e => { if (!bar.contains(e.relatedTarget as Node | null)) $("#ddrop")?.remove(); });
bar.addEventListener("drop", e => {
  $("#ddrop")?.remove();
  if (agentField() && e.dataTransfer?.files.length) {
    e.preventDefault();
    agentAttach(e.dataTransfer.files).then(() => $("#agmsg")?.focus());
    return;
  }
  if (S.view !== "chat" || !e.dataTransfer?.files.length) return;
  e.preventDefault();
  attach(e.dataTransfer.files).then(() => focusAsk());
});
