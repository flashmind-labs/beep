import { chatAsk } from "./chat-shell.tsx";
import { agentMessage } from "../agents/session-frame.tsx";
import { showFieldDrop } from "../agents/composer-ui.tsx";
import { AGENT_IMAGE_TYPES } from "../agents/agents.ts";
import { agentAttach } from "../agents/images.ts";
import { PATH_TYPE } from "../agentpane/files.ts";
import { addMention, uploadDropped } from "../agents/workdrop.ts";
import { drawQueue, focusAsk } from "./chat.ts";
import { bar, S } from "../bar/core.ts";
import { refreshHints } from "../bar/hints.ts";
import { createElement } from "react";
import { createDocumentScope } from "../ui/document-root.tsx";
import { ICON } from "../ui/icons.tsx";

// ---------- Attached images and files ----------

/// A file a prompt carried: one just sent, or one the server keeps
/// (`FileRef`), which has an id to open it by.
export interface AskFile {
  name: string;
  mime: string;
  size: number;
  id?: string;
}

/// A file waiting in the prompt, its bytes in base64 (`FileUpload`, and
/// its size).
export type Attachment = Omit<AskFile, "id"> & { data: string };

// Pictures and files pasted or dropped into the prompt, sent with the next
// message. Each picture is shrunk and sent as JPEG, as the phone does; other
// files go as they are, up to MAX_UPLOAD in all.
export const attached: { pics: string[]; files: Attachment[]; error: string; timer?: ReturnType<typeof setTimeout> } = { pics: [], files: [], error: "" };
export const MAX_IMAGES = 8;
export const MAX_UPLOAD = 5 * 1024 * 1024;
export const PIC_SIDE = 1024;

export const attachedCount = () => attached.pics.length + attached.files.length;

/// The strip of attached pictures and files above the prompt, or the error
/// from the last paste, with the placeholder and hints to match.
export function drawAttached() {
  if (!chatAsk.current) return;
  drawQueue();
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
const agentField = () => S.view === "agent" && !!agentMessage.current;

document.addEventListener("paste", e => {
  const agent = agentField();
  if (!agent && (S.view !== "chat" || !chatAsk.current)) return;
  const files = [...(e.clipboardData?.items ?? [])].filter(i => i.kind === "file").map(i => i.getAsFile()).filter((f): f is File => !!f);
  if (!files.length) return;
  e.preventDefault();
  agent ? agentDrop(files) : attach(files);
});

/// The largest image a coding agent gets as it is; a larger one is converted.
const AGENT_IMAGE_KEPT = 2 * 1024 * 1024;

/// `file` as a shrunk JPEG, for an agent that cannot take it as it is.
async function agentJpeg(file: File) {
  const data = (await shrinkPic(file)).split(",")[1];
  const name = (file.name || "image").replace(/\.[^.]*$/, "");
  return new File([Uint8Array.from(atob(data), c => c.charCodeAt(0))], `${name}.jpg`, { type: "image/jpeg" });
}

/// Files for a coding agent split into the pictures it takes, converted
/// when too big or in another format, and the rest, a picture that cannot
/// be drawn included.
export async function agentPics(files: Iterable<File>) {
  const pics: File[] = [], rest: File[] = [];

  for (const f of files) {
    if (AGENT_IMAGE_TYPES.includes(f.type) && f.size <= AGENT_IMAGE_KEPT) pics.push(f);
    else if (f.type.startsWith("image/")) await agentJpeg(f).then(jpeg => pics.push(jpeg), () => rest.push(f));
    else rest.push(f);
  }

  return { pics, rest };
}

/// Files pasted or dropped on a coding agent session: the pictures attach
/// to the message; anything else uploads into the session's folder and is
/// mentioned.
export async function agentDrop(files: Iterable<File>) {
  const { pics, rest } = await agentPics(files);
  if (rest.length) uploadDropped(rest);
  if (pics.length) await agentAttach(pics);
}

// Dropping images and files on the chat attaches them too; a path dragged
// from a session's Files tab onto its conversation is mentioned.
const dropOverlay = createDocumentScope<"drop">();

/// A drop handled elsewhere, as on the Files tab's tree, ends the overlay.
export const hideDropOverlay = () => { overlaid = false; dropOverlay.remove(["drop"]); };
let overlaid = false;
bar.addEventListener("dragover", e => {
  const agent = agentField();
  const types = e.dataTransfer?.types ?? [];

  if (agent && types.includes(PATH_TYPE)) {
    if (!(e.target instanceof Element && e.target.closest("#agconv"))) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
    showFieldDrop(true);
    return;
  }

  if ((S.view !== "chat" && !agent) || !types.includes("Files")) return;
  e.preventDefault();
  if (overlaid) return;
  overlaid = true;
  dropOverlay.set([{ slot: "drop", host: bar, content: createElement("div", { className: "ddrop", id: "ddrop" }, createElement("span", { style: { display: "contents" }, dangerouslySetInnerHTML: { __html: ICON.image } }), agent ? "Drop images to send, other files to upload" : "Drop images or files to ask about them") }]);
});
bar.addEventListener("dragleave", e => {
  const into = e.relatedTarget instanceof Element ? e.relatedTarget : null;
  if (!into || !bar.contains(into)) hideDropOverlay();
  if (!into?.closest("#agconv")) showFieldDrop(false);
});
bar.addEventListener("drop", e => {
  hideDropOverlay();
  showFieldDrop(false);
  const path = e.dataTransfer?.getData(PATH_TYPE);

  if (agentField() && path) {
    e.preventDefault();
    addMention(path);
    return;
  }

  if (agentField() && e.dataTransfer?.files.length) {
    e.preventDefault();
    agentDrop(e.dataTransfer.files).then(() => agentMessage.current?.focus());
    return;
  }

  if (S.view !== "chat" || !e.dataTransfer?.files.length) return;
  e.preventDefault();
  attach(e.dataTransfer.files).then(() => focusAsk());
});
