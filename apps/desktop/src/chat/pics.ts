import { bytesOf } from "../agentpane/media-state.ts";
import { invoke, useBuildCapabilities } from "../ui/tauri.ts";

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

export type PastedFiles = File[] | Promise<File[]>;

/// WebKit can deliver an empty event for a picture on the OS clipboard.
export function pastedFiles(event: ClipboardEvent): PastedFiles | null {
  if (event.defaultPrevented) return null;
  const data = event.clipboardData;
  const files = [...(data?.files ?? [])];
  if (files.length) return files;
  const items = [...(data?.items ?? [])].filter(item => item.kind === "file").map(item => item.getAsFile()).filter((file): file is File => file !== null);
  if (items.length) return items;
  if (!event.isTrusted || data?.types.some(type => type !== "Files") || !useBuildCapabilities.getState().image_clipboard) return null;

  return invoke<string | null>("read_clipboard_image").then(png =>
    png ? [new File([bytesOf(png)], "image.png", { type: "image/png" })] : [],
  ).catch(error => { console.warn("reading a pasted picture:", error); return []; });
}
