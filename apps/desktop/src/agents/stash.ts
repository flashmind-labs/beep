import { bindPromptStash } from "../chat/stash.ts";
import { draftText } from "../chat/compose.ts";
import { stashFile } from "../chat/stash-state.ts";
import { windowHooks } from "../bar/core.ts";
import { AG } from "./agents.ts";
import type { Attached } from "./agentqueue.ts";
import { attachPreview, drawAgentAttached, imagesReading } from "./images.ts";
import { MN, prepareStashFiles } from "./workdrop.ts";

export function bindSessionStash(field: HTMLTextAreaElement) {
  const open = AG.open;
  bindPromptStash(field, {
    project: () => AG.open === open && AG.page ? windowHooks?.sessionProject?.(AG.page.session)?.name ?? null : null,
    hasAttachments: () => !!AG.att.length || !!MN.paths.length,
    capture: async () => {
      if (imagesReading || MN.uploads.length || MN.reading || MN.ask) throw new Error("Wait for the attachments to finish loading");
      return {
        text: draftText(field.value),
        images: AG.att.filter(image => image.original === null).map(({ name, mime, size, data }) => ({ name, mime, size, data })),
        files: MN.paths.flatMap(path => { const file = MN.files.get(path); return file ? [file] : []; }),
        paths: MN.paths.filter(path => !MN.files.has(path)),
      };
    },
    clear: () => {
      field.value = ""; AG.att = []; MN.paths = []; MN.files.clear();
      drawAgentAttached();
    },
    prepare: async message => {
      message.images.forEach(stashFile);
      const files = await prepareStashFiles(message.files);
      const images: Attached[] = message.images.map(file => ({ ...file, url: `data:${file.mime};base64,${file.data}`, original: null }));
      for (const [path, file] of files) if (file.mime.startsWith("image/")) await attachPreview(images, stashFile(file), path);

      return () => {
        field.value = message.text;
        AG.att = images;
        MN.paths = [...message.paths, ...files.keys()];
        MN.files = files;
        drawAgentAttached();
        field.dispatchEvent(new Event("input", { bubbles: true }));
      };
    },
  });
}
