import { openViewer } from "../chat/media.ts";
import { ICON } from "../ui/icons.tsx";
import type { Attached } from "./agentqueue.ts";

const ids = new WeakMap<object, number>();
let next = 0;
function key(value: object) {
  let id = ids.get(value);
  if (id === undefined) { id = ++next; ids.set(value, id); }
  return id;
}

export function StartAttachmentChips({ images, files, host }: { images: Attached[]; files: { file: File }[]; host: HTMLElement }) {
  return <>{images.map((image, i) => <span className="att" key={key(image)}><button type="button" className="preview" data-att-preview={i} aria-label={`Preview ${image.name}`} title="Expand image" onClick={() => openViewer(images.map(a => ({ url: a.url, name: a.name, saves: true })), i, host.closest<HTMLElement>(".cc-main") ?? undefined)}><img src={image.url} alt="" /><span className="nm">{image.name}</span></button><button type="button" aria-label={`Remove ${image.name}`} data-startimage={i} dangerouslySetInnerHTML={{ __html: ICON.x }} /></span>)}
    {files.map(({ file }, i) => <span className="att" key={key(files[i])}><span className="nm" title="Copied to the selected machine when you start">{file.name || "file"}</span><button type="button" aria-label={`Remove ${file.name || "file"}`} data-startfile={i} dangerouslySetInnerHTML={{ __html: ICON.x }} /></span>)}</>;
}
