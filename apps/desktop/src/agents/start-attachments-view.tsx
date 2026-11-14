import { useEffect, useMemo } from "react";
import { AttachmentBatch, ImagePreview } from "./composer-ui.tsx";
import { identity } from "../ui/parts.ts";
import { Icon } from "../ui/icons.tsx";
import type { Attached } from "./agentqueue.ts";

export function StartAttachmentChips({ images, files, remove }: { images: Attached[]; files: { file: File }[]; remove(kind: "image" | "file", index: number): void }) {
  return <AttachmentBatch count={images.length + files.filter(({ file }) => !images.some(image => image.original === file)).length}>{images.map((image, i) => <span className="att" key={identity(image)}><ImagePreview images={images} index={i} listed /><button type="button" aria-label={`Remove ${image.name}`} data-startimage={i} onClick={() => remove("image", i)}><Icon name="x" /></button></span>)}
    {files.map(({ file }, i) => !images.some(image => image.original === file) && <span className="att" key={identity(files[i])}><LocalFile file={file} /><button type="button" aria-label={`Remove ${file.name || "file"}`} data-startfile={i} onClick={() => remove("file", i)}><Icon name="x" /></button></span>)}</AttachmentBatch>;
}

function LocalFile({ file }: { file: File }) {
  const url = useMemo(() => file.type.startsWith("image/") ? URL.createObjectURL(file) : null, [file]);
  useEffect(() => () => { if (url) URL.revokeObjectURL(url); }, [url]);

  return url ? <ImagePreview images={[{ url, name: file.name }]} index={0} listed={false} /> : <span className="nm">{file.name || "file"}</span>;
}
