import { createContext, useLayoutEffect, useRef, type KeyboardEvent, type MouseEvent, type ReactNode } from "react";
import type { ChatBody } from "../ui/turn.ts";
import type { ResearchAct } from "./research-state.ts";

/// What the controls of an opened research run; nothing outside one.
export const ResearchActions = createContext<(act: ResearchAct) => void>(() => {});

/// The elements the page's scrolling, composing and resizing work on.
export interface ShellParts { root: HTMLElement; shell: HTMLElement; log: ChatBody; chat: HTMLElement; ask: HTMLTextAreaElement; views: HTMLElement; divider: HTMLElement; pane: HTMLElement }

export interface ShellProps {
  head: ReactNode; chat: ReactNode; tabs: ReactNode; pane: ReactNode; notice: ReactNode;
  placeholder: string; zoomed: boolean; resizing: boolean; paneBasis: number | null;
  act(act: ResearchAct): void;
  bind(parts: ShellParts | null): void;
  click(event: MouseEvent<HTMLElement>): void;
  askKey(event: KeyboardEvent<HTMLTextAreaElement>): void;
  logScrolled(): void;
  scrolled(): void;
  resetPane(): void;
}

export function ResearchShell(p: ShellProps) {
  const root = useRef<HTMLDivElement>(null), shell = useRef<HTMLDivElement>(null), log = useRef<HTMLDivElement>(null), chat = useRef<HTMLDivElement>(null);
  const ask = useRef<HTMLTextAreaElement>(null), views = useRef<HTMLDivElement>(null), divider = useRef<HTMLDivElement>(null), pane = useRef<HTMLElement>(null);
  const { bind } = p;
  useLayoutEffect(() => {
    if (!root.current || !shell.current || !log.current || !chat.current || !ask.current || !views.current || !divider.current || !pane.current) return;
    bind({ root: root.current, shell: shell.current, log: log.current, chat: chat.current, ask: ask.current, views: views.current, divider: divider.current, pane: pane.current });
    return () => bind(null);
  }, [bind]);

  const zoomed = p.zoomed ? " zoomed" : "";
  return <ResearchActions.Provider value={p.act}><div className="rs" ref={root} onClick={p.click}>
    <div className="dhead">{p.head}</div>
    <div ref={shell} className={`agshell${zoomed}${p.resizing ? " resizing" : ""}`}>
      <div className="agconv">
        <div ref={log} className="dbody rs-log" onScroll={p.logScrolled} onScrollCapture={p.scrolled}><div className="dagmain"><div ref={chat} className="dcoderlog">{p.chat}</div></div></div>
        <div className="dinput small"><textarea ref={ask} rows={1} autoComplete="off" spellCheck={false} aria-label="Message the lead" placeholder={p.placeholder} onKeyDown={p.askKey} /></div>
      </div>
      <div ref={divider} className={`agdiv${p.resizing ? " active" : ""}`} role="separator" aria-orientation="vertical" tabIndex={0} title="Drag to resize, double-click to reset" onDoubleClick={p.resetPane} />
      <aside ref={pane} className={`agpane rs-pane${zoomed}`} style={p.paneBasis === null ? undefined : { flexBasis: `var(--drag-size, ${p.paneBasis}px)` }}>
        <div className="agpane-content"><div className="agphead"><div className="agtabs" role="tablist">{p.tabs}</div></div><div ref={views} className="agpviews" onScrollCapture={p.scrolled}>{p.pane}</div></div>
        {p.notice}
      </aside>
    </div>
  </div></ResearchActions.Provider>;
}
