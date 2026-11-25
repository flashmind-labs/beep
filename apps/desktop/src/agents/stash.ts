import { bindPromptStash } from "../chat/stash.tsx";
import { draftText } from "../chat/compose.tsx";
import { stashFile } from "../chat/stash-state.ts";
import { windowHooks } from "../bar/core.ts";
import { AG } from "./agents-state.ts";
import type { Attached } from "./agentqueue.ts";
import { attachPreview, drawAgentAttached } from "./images.tsx";
import { MN, attachmentKey, setAttached } from "./mentions.ts";

export function bindSessionStash(field: HTMLTextAreaElement) {
  const open = AG.open;
  bindPromptStash(field, {
    project: () => AG.open === open && AG.page ? windowHooks?.sessionProject?.(AG.page.session)?.name ?? null : null,
    hasAttachments: () => !!AG.att.length || !!MN.paths.length,
    capture: async () => {
      if (MN.uploads.length || MN.reading || MN.ask) throw new Error("Wait for the attachments to finish loading");
      return {
        text: draftText(field.value),
        images: AG.att.filter(image => image.original === null).map(({ name, mime, size, data }) => ({ name, mime, size, data })),
        files: MN.paths.flatMap(path => { const file = MN.files.get(path); return file ? [file] : []; }),
        paths: MN.paths.filter(path => !MN.files.has(path)),
      };
    },
    clear: () => {
      field.value = ""; setAttached([]); MN.paths = []; MN.files.clear();
      drawAgentAttached();
    },
    prepare: async message => {
      message.images.forEach(stashFile);
      message.files.forEach(stashFile);
      const files = new Map(message.files.map(file => [attachmentKey(file.name), file]));
      const images: Attached[] = message.images.map(file => ({ ...file, url: `data:${file.mime};base64,${file.data}`, original: null }));
      for (const [path, file] of files) if (file.mime.startsWith("image/")) await attachPreview(images, stashFile(file), path);

      return () => {
        field.value = message.text;
        setAttached(images);
        MN.paths = [...message.paths, ...files.keys()];
        MN.files = files;
        drawAgentAttached();
        field.dispatchEvent(new Event("input", { bubbles: true }));
      };
    },
  });
}
