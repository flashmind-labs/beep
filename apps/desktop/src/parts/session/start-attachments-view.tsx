import { useEffect, useMemo } from "react";
import { AttachmentBatch, ImagePreview, RemoveChip } from "./composer-ui.tsx";
import { identity } from "../../ui/parts.tsx";
import { ProgressBar, Spinner } from "../../ui/kit.tsx";
import { previewsOf } from "./images.tsx";
import type { Attached } from "../../stores/agents/agentqueue.ts";
import type { PendingFile } from "../../stores/agents/start-files.ts";
import { speedNote } from "./attachments-view.tsx";

export function StartAttachmentChips({ images, files, progress, remove }: { images: Attached[]; files: PendingFile[]; progress: { size: number; done: number } | null; remove(kind: "image" | "file", index: number): void }) {
  const loose = (file: File) => !images.some(image => image.original === file);
  const uploaded = progress?.size ? Math.round(progress.done / progress.size * 100) : 0;

  return <>{progress && <ProgressBar pct={uploaded} label="Uploading" />}
    <AttachmentBatch count={images.length + files.filter(({ file }) => loose(file)).length}>{images.map((image, i) => <span className="att" key={identity(image)}><ImagePreview images={previewsOf(images)} index={i} listed /><RemoveChip name={image.name} data="data-startimage" index={i} remove={() => remove("image", i)} /></span>)}
    {files.map((entry, i) => loose(entry.file) && <span className="att" key={identity(entry)} title={entry.error ?? undefined}><LocalFile entry={entry} /><RemoveChip name={entry.file.name || "file"} data="data-startfile" index={i} remove={() => remove("file", i)} /></span>)}</AttachmentBatch></>;
}

function LocalFile({ entry: { file, at, path, done, error, started } }: { entry: PendingFile }) {
  const url = useMemo(() => file.type.startsWith("image/") ? URL.createObjectURL(file) : null, [file]);
  useEffect(() => () => { if (url) URL.revokeObjectURL(url); }, [url]);
  const uploading = at && !path && !error;

  return <>{uploading && <Spinner />}{url ? <ImagePreview images={[{ url, name: file.name, full: null }]} index={0} listed={false} /> : <span className="nm">{file.name || "file"}</span>}{uploading && <small>{file.size ? Math.round(done / file.size * 100) : 0}%{speedNote({ started, done, size: file.size })}</small>}</>;
}
