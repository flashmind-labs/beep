import { agentAttach } from "./agents.ts";
import { focusAsk } from "./chat.ts";
import { $, bar, esc, h, ICON, invoke, S } from "./core.ts";
import { refreshHints } from "./hints.ts";
import { VOICE_ICON } from "./live.ts";
import { fileKind, fileSize, GONE } from "./media.ts";

// ---------- Attached images and files ----------

// Pictures and files pasted or dropped into the prompt, sent with the next
// message. Each picture is shrunk and sent as JPEG, as the phone does; other
// files go as they are, up to MAX_UPLOAD in all.
export const attached = { pics: [], files: [], error: "" };
export const MAX_IMAGES = 8;
export const MAX_UPLOAD = 5 * 1024 * 1024;
export const PIC_SIDE = 1024;

export const attachedCount = () => attached.pics.length + attached.files.length;

/// An attached file's chip: its name and size, with a remove button while
/// it waits in the prompt, as a button that opens it once it is kept.
export function fileChip(f, i) {
  // A spoken message's recording, kept with it.
  if (i === undefined && f.mime?.startsWith("audio/")) {
    const tag = f.id ? "button" : "span";
    const about = ["Audio", fileSize(f.size)].filter(Boolean).join(", ");
    return `<${tag} class="dachip" aria-label="Open your recording"><span class="ic">${VOICE_ICON.mic}</span><span style="min-width:0"><span class="nm">Your recording</span><span class="sz">${esc(about)}</span></span></${tag}>`;
  }
  const about = [fileKind(f), fileSize(f.size)].filter(Boolean).join(", ");
  const remove = i === undefined ? "" : `<button aria-label="Remove ${esc(f.name)}" data-i="${i}">${ICON.x}</button>`;
  const tag = f.id ? "button" : "span";
  return `<${tag} class="dachip"><span class="ic">${ICON.doc}</span><span style="min-width:0"><span class="nm">${esc(f.name)}</span><span class="sz">${esc(about)}</span></span>${remove}</${tag}>`;
}

/// The files a prompt carried, under it. A kept one opens like a file Beep sent.
export function askFiles(files) {
  const row = h(`<div class="dachips">${files.map(f => fileChip(f)).join("")}</div>`);
  [...row.children].forEach((chip, i) => {
    const file = files[i];
    if (!file.id) return;
    chip.addEventListener("click", async () => {
      if (chip.classList.contains("gone")) return;
      try { await invoke("open_file", { id: file.id, name: file.name }); }
      catch (e) {
        chip.querySelector(".sz").textContent = String(e) === GONE ? GONE : String(e);
        if (String(e) === GONE) chip.classList.add("gone");
      }
    });
  });
  return row;
}

/// The strip of attached pictures and files above the prompt, or the error
/// from the last paste, with the placeholder and hints to match.
export function drawAttached() {
  const old = $("#thumbs"), input = $("#ask")?.closest(".dinput");
  if (!input) return;
  const pics = attached.pics.length, n = attachedCount();
  const el = n || attached.error ? h(`<div class="dthumbs" id="thumbs">${attached.pics.map((url, i) =>
    `<span class="dthumb"><img src="${esc(url)}" alt="Image ${i + 1}"><button aria-label="Remove image ${i + 1}" data-i="${i}">${ICON.x}</button></span>`).join("")}${attached.files.map((f, i) => fileChip(f, pics + i)).join("")}${attached.error ? `<span class="derr">${esc(attached.error)}</span>` : ""}</div>`) : null;
  el?.querySelectorAll("button").forEach(b => b.addEventListener("click", () => { unattach(+b.dataset.i); focusAsk(); }));
  if (old) el ? old.replaceWith(el) : old.remove();
  else if (el) input.before(el);
  const ask = $("#ask");
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
export function unattach(i) {
  if (i < attached.pics.length) attached.pics.splice(i, 1);
  else attached.files.splice(i - attached.pics.length, 1);
  attached.error = "";
  drawAttached();
}

export function attachError(text) {
  attached.error = text;
  drawAttached();
  clearTimeout(attached.timer);
  attached.timer = setTimeout(() => { attached.error = ""; drawAttached(); }, 4000);
}

/// Adds files from a paste or a drop: pictures shrunk, up to `MAX_IMAGES`;
/// other files, or pictures that cannot be drawn, as they are.
export async function attach(files) {
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
export function fileBase64(file) {
  return new Promise((done, fail) => {
    const reader = new FileReader();
    reader.onload = () => done(String(reader.result).split(",")[1] ?? "");
    reader.onerror = () => fail(reader.error);
    reader.readAsDataURL(file);
  });
}

/// `file` as a JPEG data URI no longer than `PIC_SIDE` on its longest side.
export async function shrinkPic(file) {
  const img = await createImageBitmap(file);
  const scale = Math.min(1, PIC_SIDE / Math.max(img.width, img.height, 1));
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(img.width * scale));
  canvas.height = Math.max(1, Math.round(img.height * scale));
  const cx = canvas.getContext("2d");
  // JPEG has no transparency; a screenshot's clear parts become white, not black.
  cx.fillStyle = "#fff";
  cx.fillRect(0, 0, canvas.width, canvas.height);
  cx.drawImage(img, 0, 0, canvas.width, canvas.height);
  img.close();
  return canvas.toDataURL("image/jpeg", 0.8);
}

document.addEventListener("paste", e => {
  if (S.view === "agent" && $("#agmsg")) {
    const files = [...(e.clipboardData?.items ?? [])].filter(i => i.kind === "file").map(i => i.getAsFile()).filter(Boolean);
    if (files.length) { e.preventDefault(); agentAttach(files); }
    return;
  }
  if (S.view !== "chat" || !$("#ask")) return;
  const files = [...(e.clipboardData?.items ?? [])].filter(i => i.kind === "file").map(i => i.getAsFile()).filter(Boolean);
  if (files.length) { e.preventDefault(); attach(files); }
});

// Dropping images and files on the chat attaches them too.
bar.addEventListener("dragover", e => {
  const agent = S.view === "agent" && $("#agmsg");
  if ((S.view !== "chat" && !agent) || !e.dataTransfer?.types.includes("Files")) return;
  e.preventDefault();
  if (!$("#ddrop")) bar.append(h(`<div class="ddrop" id="ddrop">${ICON.image}${agent ? "Drop images to send them to the agent" : "Drop images or files to ask about them"}</div>`));
});
bar.addEventListener("dragleave", e => { if (!bar.contains(e.relatedTarget)) $("#ddrop")?.remove(); });
bar.addEventListener("drop", e => {
  $("#ddrop")?.remove();
  if (S.view === "agent" && $("#agmsg") && e.dataTransfer?.files.length) {
    e.preventDefault();
    agentAttach(e.dataTransfer.files).then(() => $("#agmsg")?.focus());
    return;
  }
  if (S.view !== "chat" || !e.dataTransfer?.files.length) return;
  e.preventDefault();
  attach(e.dataTransfer.files).then(() => focusAsk());
});

