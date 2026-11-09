import { createElement } from "react";
import { agentPics, fileBase64 } from "../chat/attach.ts";
import { draftText } from "../chat/compose.ts";
import { bindPromptStash } from "../chat/stash.ts";
import { stashFile } from "../chat/stash-state.ts";
import type { Attached } from "./agentqueue.ts";
import { attachImages } from "./images.ts";
import { StartAttachmentChips } from "./start-attachments-view.tsx";
import { filePicker } from "./file-picker.ts";
import type { makeStartComposer } from "./start-composer.ts";
import { StartFiles } from "./start-files.ts";

export function startAttachments(c: ReturnType<typeof makeStartComposer>, error: (text: string) => void, changed: () => void) {
  const images: Attached[] = [];
  const files = new StartFiles();
  let reading = 0, locked = false;
  let serial = Promise.resolve();
  const draw = () => {
    c.setAttachments(images.length || files.pending.length
      ? createElement(StartAttachmentChips, { images: [...images], files: [...files.pending], host: c.att })
      : null);
  };
  const add = (input: Iterable<File>) => {
    if (locked) return Promise.resolve();
    const dropped = [...input];
    error("");
    reading++;
    changed();
    serial = serial.then(async () => {
      const { pics, rest } = await agentPics(dropped);
      files.add(rest, error);
      await attachImages(images, pics, error);
      draw();
    }).catch(e => error(String(e))).finally(() => { reading--; changed(); });
    return serial;
  };
  const picker = filePicker(add);
  c.send.before(picker.button, picker.input);
  c.att.addEventListener("click", e => {
    if (locked) return;
    const button = (e.target as HTMLElement).closest<HTMLElement>("[data-startimage], [data-startfile]");
    if (!button) return;
    if (button.dataset.startimage !== undefined) images.splice(Number(button.dataset.startimage), 1);
    else files.pending.splice(Number(button.dataset.startfile), 1);
    draw();
    c.input.focus();
  });
  c.input.addEventListener("paste", e => {
    const files = [...(e.clipboardData?.items ?? [])].filter(i => i.kind === "file").map(i => i.getAsFile()).filter((f): f is File => !!f);
    if (!files.length) return;
    e.preventDefault();
    e.stopPropagation();
    void add(files);
  });
  c.form.addEventListener("dragover", e => {
    if (!e.dataTransfer?.types.includes("Files")) return;
    e.preventDefault();
    e.stopPropagation();
    e.dataTransfer.dropEffect = locked ? "none" : "copy";
    if (!locked) c.box.classList.add("dropping");
  });
  c.form.addEventListener("dragleave", e => { if (!c.form.contains(e.relatedTarget as Node | null)) c.box.classList.remove("dropping"); });
  c.form.addEventListener("drop", e => {
    c.box.classList.remove("dropping");
    if (!e.dataTransfer?.files.length) return;
    e.preventDefault();
    e.stopPropagation();
    void add(e.dataTransfer.files).then(() => c.input.focus());
  });
  return {
    add, images, files,
    get reading() { return reading > 0; },
    get count() { return images.length + files.pending.length; },
    setBusy(value: boolean) { locked = value; picker.button.disabled = value; },
    clear() { images.length = 0; files.pending.length = 0; draw(); },
    bindStash(project: () => string | null) {
      let sendDisabled = false;
      bindPromptStash(c.input, {
        project,
        hasAttachments: () => !!images.length || !!files.pending.length,
        lock: value => {
          if (value) sendDisabled = c.send.disabled;
          locked = value; picker.button.disabled = value;
          c.send.disabled = value || sendDisabled;
        },
        capture: async () => {
          if (reading) throw new Error("Wait for the attachments to finish loading");
          return { text: draftText(c.input.value), images: images.map(({ name, mime, size, data }) => ({ name, mime, size, data })), paths: [], files: await Promise.all(files.pending.map(async ({ file }) => ({ name: file.name, mime: file.type, size: file.size, data: await fileBase64(file) }))) };
        },
        clear: () => { c.input.value = ""; images.length = 0; files.pending.length = 0; draw(); c.input.dispatchEvent(new Event("input", { bubbles: true })); },
        prepare: async message => {
          const restoredFiles = message.files.map(stashFile);
          message.images.forEach(stashFile);
          return () => {
            c.input.value = [message.text, ...message.paths.map(path => `@${path}`)].filter(Boolean).join("\n");
            images.splice(0, images.length, ...message.images.map(file => ({ ...file, url: `data:${file.mime};base64,${file.data}` })));
            files.pending.splice(0, files.pending.length, ...restoredFiles.map(file => ({ file })));
            draw(); c.input.dispatchEvent(new Event("input", { bubbles: true }));
          };
        },
      });
    },
  };
}
