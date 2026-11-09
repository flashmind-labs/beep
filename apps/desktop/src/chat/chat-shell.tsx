import { useLayoutEffect, useRef } from "react";
import { ICON } from "../ui/icons.ts";
import { chatBodyHost } from "./work-view.tsx";
import { fileKind, fileSize } from "./media.ts";
import type { Attachment } from "./attach.ts";
import type { ChatBody } from "../ui/turn.ts";

interface Attachments { pics: string[]; files: Attachment[]; error: string }
interface Queue { list: readonly string[]; held: boolean }
export function ChatShell({ body, hints, queue, attachments, unattach, edit, remove, send }: { body: ChatBody; hints: string; queue: Queue | null; attachments: Attachments; unattach(index: number): void; edit(index: number): void; remove(index: number): void; send(): void }) {
  const host = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const current = host.current;
    if (!current) return;
    const content = chatBodyHost(body);
    current.append(content);
    return () => { content.remove(); };
  }, [body]);
  const seen = new Map<string, number>();
  return <>{(attachments.pics.length > 0 || attachments.files.length > 0 || attachments.error) && <div className="dthumbs" id="thumbs">{attachments.pics.map((url, index) => <span className="dthumb" key={`${url}:${index}`}><img src={url} alt={`Image ${index + 1}`} /><button aria-label={`Remove image ${index + 1}`} data-i={index} onClick={() => unattach(index)} dangerouslySetInnerHTML={{ __html: ICON.x }} /></span>)}{attachments.files.map((file, index) => <span className="dachip" key={`${file.name}:${index}`}><span className="ic" dangerouslySetInnerHTML={{ __html: ICON.doc }} /><span className="dcol0"><span className="nm">{file.name}</span><span className="sz">{[fileKind(file), fileSize(file.size)].filter(Boolean).join(", ")}</span></span><button aria-label={`Remove ${file.name}`} data-i={attachments.pics.length + index} onClick={() => unattach(attachments.pics.length + index)} dangerouslySetInnerHTML={{ __html: ICON.x }} /></span>)}{attachments.error && <span className="derr">{attachments.error}</span>}</div>}<div className="dinput dgrow"><textarea id="ask" rows={1} placeholder="Ask Beep…" autoComplete="off" spellCheck={false} /><div className="dgrip" id="grip" role="separator" aria-orientation="horizontal" aria-label="Resize the prompt" title="Drag to resize, double-click to reset" /></div><div ref={host} style={{ display: "contents" }} />{queue && !!queue.list.length && <div className="ag-queue" id="queue">{queue.list.map((text, index) => {
    const occurrence = seen.get(text) ?? 0; seen.set(text, occurrence + 1);
    return <div className="it" key={`${text}\0${occurrence}`}><span className="lbl">Queued</span><span className="t" title={text}>{text}</span><span className="w">{queue.held ? "not sent" : "after the reply"}</span>{queue.held && !index && <button data-q="send" onClick={send}>Send</button>}<button data-q={index} data-act="edit" onClick={() => edit(index)}>Edit</button><button data-q={index} aria-label="Remove" onClick={() => remove(index)} dangerouslySetInnerHTML={{ __html: ICON.x }} /></div>;
  })}</div>}<div className="dhints" dangerouslySetInnerHTML={{ __html: hints }} /></>;
}
