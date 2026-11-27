import { fileBase64, type PastedFiles } from "../../stores/chat/pics.ts";
import { draftText } from "./compose.tsx";
import { bindPromptStash } from "./stash.tsx";
import { stashFile } from "../../stores/chat/stash-state.ts";
import type { Attached } from "../../stores/agents/agentqueue.ts";
import { attachPreview } from "./images.tsx";
import { StartAttachmentChips } from "./start-attachments-view.tsx";
import { FilePicker } from "./file-picker.tsx";
import type { StartComposer } from "./start-composer.tsx";
import { StartFiles } from "../../stores/agents/start-files.ts";

export function startAttachments(c: StartComposer, error: (text: string) => void, changed: () => void) {
  const images: Attached[] = [];
  const files = new StartFiles(() => draw(), error);
  let reading = 0, starting = false, stashing = false;
  const locked = () => starting || stashing;
  let serial = Promise.resolve();
  const remove = (kind: "image" | "file", index: number) => {
    if (locked()) return;
    if (kind === "image") {
      const [image] = images.splice(index, 1);
      const at = files.pending.findIndex(entry => entry.file === image.original);
      if (at >= 0) files.remove(at);
    } else files.remove(index);
    draw();
    c.input.focus();
  };
  const draw = () => {
    c.update({ attachmentView: images.length || files.pending.length
      ? <StartAttachmentChips images={[...images]} files={[...files.pending]} progress={files.progress} remove={remove} />
      : null });
  };
  const add = (input: PastedFiles) => {
    if (locked()) return Promise.resolve();
    const field = c.input;
    error("");
    reading++;
    changed();
    serial = serial.then(async () => {
      const dropped = await input;
      if (!field.isConnected || !dropped.length) return;
      files.add(dropped);
      for (const file of dropped) if (file.type.startsWith("image/") && files.pending.some(entry => entry.file === file)) await attachPreview(images, file, file);
      draw();
    }).catch(e => error(String(e))).finally(() => { reading--; changed(); });
    return serial;
  };
  const drawPicker = () => c.update({ uploadPicker: <FilePicker attach={add} disabled={locked()} /> });
  drawPicker();
  return {
    add, images, files,
    /// Files pasted or dropped on the composer, which keeps the focus.
    take(dropped: PastedFiles) {
      const field = c.input;
      void add(dropped).then(() => { if (field.isConnected) field.focus(); });
    },
    /// Takes the last chip off the composer.
    removeLast() {
      const loose = files.pending.findLastIndex(entry => !images.some(image => image.original === entry.file));
      if (loose >= 0) remove("file", loose);
      else if (images.length) remove("image", images.length - 1);
    },
    get reading() { return reading > 0; },
    get count() { return images.length + files.pending.length; },
    setBusy(value: boolean) { starting = value; drawPicker(); },
    /// After a start: the agent reads the uploaded files, so they stay.
    clear() { images.length = 0; files.forget(); draw(); },
    /// The draft is abandoned: its uploads are deleted.
    discard() { images.length = 0; files.discardAll(); draw(); },
    bindStash(project: () => string | null) {
      bindPromptStash(c.input, {
        canUse: () => !starting,
        project,
        hasAttachments: () => !!images.length || !!files.pending.length,
        lock: value => {
          stashing = value; drawPicker();
          c.update({ stashLocked: value });
        },
        capture: async () => {
          if (reading) throw new Error("Wait for the attachments to finish loading");
          return { text: draftText(c.input.value), images: images.filter(image => image.original === null).map(({ name, mime, size, data }) => ({ name, mime, size, data })), paths: [], files: await Promise.all(files.pending.map(async ({ file }) => ({ name: file.name, mime: file.type, size: file.size, data: await fileBase64(file) }))) };
        },
        clear: () => { c.setText(""); images.length = 0; files.discardAll(); draw(); c.input.dispatchEvent(new Event("input", { bubbles: true })); },
        prepare: async message => {
          const restoredFiles = message.files.map(stashFile);
          message.images.forEach(stashFile);
          const restored: Attached[] = message.images.map(file => ({ ...file, url: `data:${file.mime};base64,${file.data}`, original: null }));
          for (const file of restoredFiles) if (file.type.startsWith("image/")) await attachPreview(restored, file, file);

          return () => {
            c.setText([message.text, ...message.paths.map(path => `@${path}`)].filter(Boolean).join("\n"));
            images.splice(0, images.length, ...restored);
            files.discardAll();
            files.add(restoredFiles);
            draw(); c.input.dispatchEvent(new Event("input", { bubbles: true }));
          };
        },
      });
    },
  };
}
