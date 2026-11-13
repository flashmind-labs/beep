import { useEffect, useMemo } from "react";
import { AttachmentBatch } from "./composer-ui.tsx";
import { openViewer } from "../chat/media.ts";
import { Icon } from "../ui/icons.tsx";
import type { Attached } from "./agentqueue.ts";

const ids = new WeakMap<object, number>();
let next = 0;
function key(value: object) {
  let id = ids.get(value);
  if (id === undefined) { id = ++next; ids.set(value, id); }
  return id;
}

export function StartAttachmentChips({ images, files, remove }: { images: Attached[]; files: { file: File }[]; remove(kind: "image" | "file", index: number): void }) {
  return <AttachmentBatch count={images.length + files.filter(({ file }) => !images.some(image => image.original === file)).length}>{images.map((image, i) => <span className="att" key={key(image)}><button type="button" className="preview" data-att-preview={i} aria-label={`Preview ${image.name}`} title="Expand image" onClick={event => openViewer(images.map(a => ({ url: a.url, name: a.name, saves: true })), i, event.currentTarget.closest<HTMLElement>(".attachment-window") ?? undefined)}><img src={image.url} alt="" /><span className="nm">{image.name}</span></button><button type="button" aria-label={`Remove ${image.name}`} data-startimage={i} onClick={() => remove("image", i)}><Icon name="x" /></button></span>)}
    {files.map(({ file }, i) => !images.some(image => image.original === file) && <span className="att" key={key(files[i])}><LocalFile file={file} /><button type="button" aria-label={`Remove ${file.name || "file"}`} data-startfile={i} onClick={() => remove("file", i)}><Icon name="x" /></button></span>)}</AttachmentBatch>;
}

function LocalFile({ file }: { file: File }) {
  const url = useMemo(() => file.type.startsWith("image/") ? URL.createObjectURL(file) : null, [file]);
  useEffect(() => () => { if (url) URL.revokeObjectURL(url); }, [url]);

  return url ? <button type="button" className="preview" aria-label={`Preview ${file.name}`} onClick={event => openViewer([{ url, name: file.name, saves: true }], 0, event.currentTarget.closest<HTMLElement>(".attachment-window") ?? undefined)}><img src={url} alt="" loading="lazy" /><span className="nm">{file.name}</span></button> : <span className="nm">{file.name || "file"}</span>;
}
