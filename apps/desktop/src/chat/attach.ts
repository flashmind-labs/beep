import { agentMessage } from "../agents/session-frame.tsx";
import { showFieldDrop } from "../agents/composer-ui.tsx";
import { PATH_TYPE } from "../agentpane/files.ts";
import { paneSession } from "../agentpane/media.ts";
import { addMention, uploadDropped } from "../agents/workdrop.ts";
import { bar, S } from "../bar/core.ts";
import { createElement } from "react";
import { createDocumentScope } from "../ui/document-root.tsx";
import { ICON } from "../ui/icons.tsx";

// ---------- Images and files dropped or pasted on a session ----------

/// The longest side of the preview a coding agent gets of a picture; the
/// picture itself goes as a file.
const PREVIEW_SIDE = 512;

/// `file`'s bytes in base64. A folder fails to read.
export function fileBase64(file: Blob) {
  return new Promise<string>((done, fail) => {
    const reader = new FileReader();
    reader.onload = () => done(String(reader.result).split(",")[1] ?? "");
    reader.onerror = () => fail(reader.error);
    reader.readAsDataURL(file);
  });
}

/// A small JPEG of the picture `file`, for an agent to see at once. Fails
/// for a picture that cannot be drawn.
export async function picPreview(file: File) {
  const img = await createImageBitmap(file);
  const scale = Math.min(1, PREVIEW_SIDE / Math.max(img.width, img.height, 1));
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(img.width * scale));
  canvas.height = Math.max(1, Math.round(img.height * scale));
  const cx = canvas.getContext("2d")!;
  // JPEG has no transparency; a screenshot's clear parts become white, not black.
  cx.fillStyle = "#fff";
  cx.fillRect(0, 0, canvas.width, canvas.height);
  cx.drawImage(img, 0, 0, canvas.width, canvas.height);
  img.close();

  const data = canvas.toDataURL("image/jpeg", 0.7).split(",")[1];
  const name = (file.name || "image").replace(/\.[^.]*$/, "");
  return new File([Uint8Array.from(atob(data), c => c.charCodeAt(0))], `${name}.jpg`, { type: "image/jpeg" });
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

/// Files pasted or dropped on a coding agent session upload to its machine
/// and are mentioned; a picture also attaches a small preview to the message.
export async function agentDrop(files: Iterable<File>) {
  await uploadDropped([...files], null, paneSession()?.cwd ?? "", true);
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
