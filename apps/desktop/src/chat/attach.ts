import { agentMessage } from "../agents/session-frame.tsx";
import { showFieldDrop } from "../agents/composer-ui.tsx";
import { AGENT_IMAGE_TYPES } from "../agents/agents.ts";
import { agentAttach } from "../agents/images.ts";
import { PATH_TYPE } from "../agentpane/files.ts";
import { addMention, uploadDropped } from "../agents/workdrop.ts";
import { bar, S } from "../bar/core.ts";
import { createElement } from "react";
import { createDocumentScope } from "../ui/document-root.tsx";
import { ICON } from "../ui/icons.tsx";

// ---------- Images and files dropped or pasted on a session ----------

/// The longest side of a picture a coding agent gets converted.
const PIC_SIDE = 1024;

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
async function shrinkPic(file: Blob) {
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
  if (!agentField()) return;
  const files = [...(e.clipboardData?.items ?? [])].filter(i => i.kind === "file").map(i => i.getAsFile()).filter((f): f is File => !!f);
  if (!files.length) return;
  e.preventDefault();
  agentDrop(files);
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

// Dropping images and files on a session attaches them too; a path dragged
// from its Files tab onto its conversation is mentioned.
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

  if (!agent || !types.includes("Files")) return;
  e.preventDefault();
  if (overlaid) return;
  overlaid = true;
  dropOverlay.set([{ slot: "drop", host: bar, content: createElement("div", { className: "ddrop", id: "ddrop" }, createElement("span", { style: { display: "contents" }, dangerouslySetInnerHTML: { __html: ICON.image } }), "Drop images to send, other files to upload") }]);
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
  }
});
