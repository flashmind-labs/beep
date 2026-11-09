import { useLayoutEffect, useRef } from "react";
import { ICON } from "../ui/icons.ts";
import type { ChatBody } from "../ui/turn.ts";

interface Queue { list: readonly string[]; held: boolean }
export function ChatShell({ body, hints, queue, edit, remove, send }: { body: ChatBody; hints: string; queue: Queue | null; edit(index: number): void; remove(index: number): void; send(): void }) {
  const host = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const current = host.current;
    if (!current) return;
    current.append(body);
    return () => { body.remove(); };
  }, [body]);
  const seen = new Map<string, number>();
  return <><div className="dinput dgrow"><textarea id="ask" rows={1} placeholder="Ask Beep…" autoComplete="off" spellCheck={false} /><div className="dgrip" id="grip" role="separator" aria-orientation="horizontal" aria-label="Resize the prompt" title="Drag to resize, double-click to reset" /></div><div ref={host} style={{ display: "contents" }} />{queue && !!queue.list.length && <div className="ag-queue" id="queue">{queue.list.map((text, index) => {
    const occurrence = seen.get(text) ?? 0; seen.set(text, occurrence + 1);
    return <div className="it" key={`${text}\0${occurrence}`}><span className="lbl">Queued</span><span className="t" title={text}>{text}</span><span className="w">{queue.held ? "not sent" : "after the reply"}</span>{queue.held && !index && <button data-q="send" onClick={send}>Send</button>}<button data-q={index} data-act="edit" onClick={() => edit(index)}>Edit</button><button data-q={index} aria-label="Remove" onClick={() => remove(index)} dangerouslySetInnerHTML={{ __html: ICON.x }} /></div>;
  })}</div>}<div className="dhints" dangerouslySetInnerHTML={{ __html: hints }} /></>;
}
