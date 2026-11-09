import { useLayoutEffect, useRef } from "react";
import type { ChatBody } from "../ui/turn.ts";

export function ChatShell({ body, hints }: { body: ChatBody; hints: string }) {
  const host = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const current = host.current;
    if (!current) return;
    current.append(body);
    return () => { body.remove(); };
  }, [body]);
  return <><div className="dinput dgrow"><textarea id="ask" rows={1} placeholder="Ask Beep…" autoComplete="off" spellCheck={false} /><div className="dgrip" id="grip" role="separator" aria-orientation="horizontal" aria-label="Resize the prompt" title="Drag to resize, double-click to reset" /></div><div ref={host} style={{ display: "contents" }} /><div className="dhints" dangerouslySetInnerHTML={{ __html: hints }} /></>;
}
