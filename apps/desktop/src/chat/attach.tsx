import { agentMessage } from "../agents/session-frame.tsx";
import { showFieldDrop } from "../agents/composer-ui.tsx";
import { PATH_TYPE } from "../agentpane/files.ts";
import { paneSession } from "../agentpane/media.ts";
import { addMention, uploadDropped } from "../agents/workdrop.ts";
import { bar, S } from "../bar/core.ts";
import { createDocumentScope } from "../ui/document-root.tsx";
import { ICON } from "../ui/icons.tsx";

// ---------- Images and files dropped or pasted on a session ----------

// Match the node's per-image and per-message byte limits.
export const AGENT_IMAGE_BYTES = 5 * 1024 * 1024;
export const AGENT_IMAGES_BYTES = 7 * 1024 * 1024;

/// `file`'s bytes in base64. A folder fails to read.
export function fileBase64(file: Blob) {
  return new Promise<string>((done, fail) => {
    const reader = new FileReader();
    reader.onload = () => done(String(reader.result).split(",")[1] ?? "");
    reader.onerror = () => fail(reader.error);
    reader.readAsDataURL(file);
  });
}

/// Preserve supported images byte for byte. Only oversized or unsupported
/// images are encoded, shrinking dimensions until they fit the byte budget.
export async function picPreview(file: File, maxBytes = AGENT_IMAGE_BYTES) {
  if (file.size <= maxBytes && ["image/png", "image/jpeg", "image/gif", "image/webp"].includes(file.type)) return file;

  const img = await createImageBitmap(file);

  try {
    const canvas = document.createElement("canvas");
    let scale = 1;

    for (;;) {
      canvas.width = Math.max(1, Math.round(img.width * scale));
      canvas.height = Math.max(1, Math.round(img.height * scale));
      const cx = canvas.getContext("2d");
      if (!cx) throw new Error("Could not prepare the image");
      cx.fillStyle = "#fff";
      cx.fillRect(0, 0, canvas.width, canvas.height);
      cx.drawImage(img, 0, 0, canvas.width, canvas.height);
      const blob = await new Promise<Blob | null>(done => canvas.toBlob(done, "image/jpeg", 0.92));
      if (!blob) throw new Error("Could not encode the image");
      if (blob.size <= maxBytes) {
        const name = (file.name || "image").replace(/\.[^.]*$/, "");
        return new File([blob], `${name}.jpg`, { type: "image/jpeg" });
      }

      if (canvas.width === 1 && canvas.height === 1) throw new Error("The image cannot fit in this message");
      scale *= Math.min(0.85, Math.sqrt(maxBytes / blob.size) * 0.95);
    }
  } finally {
    img.close();
  }
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
/// and are mentioned; a picture also attaches inline to the message.
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
  dropOverlay.set([{ slot: "drop", host: bar, content: <div className="ddrop" id="ddrop"><span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: ICON.image }} />Drop images to send, other files to upload</div> }]);
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
