import { HintView, type HintViewModel } from "../bar/hints-view.tsx";
import { Fragment, useLayoutEffect, useRef, useState, type MouseEvent } from "react";
import { mark } from "../chat/chats.ts";
import { MEM_ICON, MEM_SOURCE, scopeTag, shortDate, type Memory, type MemoryScope } from "./memories.ts";
import { ICON } from "../ui/icons.ts";

interface Filter { key: string; label: string; about?: string; ids?: Set<string>; read?: boolean }
export function MemoriesShell({ filters, picked, count, hints, search, filter, click, menu, memories, selected, mode, scopes, query, today, loaded, total, error }: {
  filters: Filter[]; picked: string; count: string; hints: HintViewModel;
  search(value: string): void; filter(key: string): void; click(event: globalThis.MouseEvent): void; menu(event: globalThis.MouseEvent): void;
  memories: Memory[]; selected: number; mode: "list" | "edit" | "clear"; scopes: Map<string, MemoryScope>; query: string; today: string; loaded: boolean; total: number; error: string;
}) {
  const input = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => { input.current?.focus(); }, []);
  const firstRead = filters.findIndex(item => item.read);
  const choose = (key: string) => { filter(key); input.current?.focus(); };
  return <><div className="dinput"><input ref={input} id="mfind" placeholder="Search memories" autoComplete="off" spellCheck={false} onChange={event => search(event.currentTarget.value)} /><span className="dmuted" id="mcount">{count}</span><kbd>⌘M</kbd></div>
    <div id="mchips" className="dtop">{filters.length > 0 && <><div className="dchips dchipbar">{filters.map((item, i) => <span key={item.key} style={{ display: "contents" }}>{i === firstRead && <span className="dsep" />}<button className="dchip" aria-pressed={item.key === picked} data-mf={item.key} onClick={() => choose(item.key)}>{item.label}{item.ids && <> <span className="dmut">{item.ids.size}</span></>}</button></span>)}</div>{filters.find(item => item.key === picked)?.about && <div className="dmuted dpadx">{filters.find(item => item.key === picked)?.about}</div>}</>}</div>
    <div className="dlist top" id="mlist" role="listbox" aria-label="Memories" onClick={(event: MouseEvent) => click(event.nativeEvent)} onContextMenu={(event: MouseEvent) => menu(event.nativeEvent)}>
      {error && <div className="derr dpad">{error}</div>}
      {mode === "clear" ? <div className="dcard dm8"><div className="dask"><b className="dtitle">Forget everything?</b><div className="dmuted dblock">Beep will forget all {total} memories. This cannot be undone.</div><div className="dline"><button className="dbtn warn" data-m="clear-yes">Forget all</button><button className="dbtn quiet" data-m="list">Cancel</button></div></div></div>
        : loaded && !total ? <div className="dempty"><span className="dicon" dangerouslySetInnerHTML={{ __html: MEM_ICON }} /><b className="dbig">Nothing remembered yet.</b><span className="dmuted dblock">Beep learns as you chat, or tell it "remember that…"</span></div>
        : <><MemoryRows {...{ memories, selected, mode, scopes, query, today, picked }} />{total > 0 && !query && picked === "all" && <div className="dpad"><button className="dbtn warn" data-m="clear">Forget all…</button></div>}</>}
    </div>
    <div id="mhints"><HintView value={hints} /></div>
  </>;
}

function MemoryRows({ memories, selected, mode, scopes, query, today, picked }: { memories: Memory[]; selected: number; mode: "list" | "edit"; scopes: Map<string, MemoryScope>; query: string; today: string; picked: string }) {
  const token = `${query}\0${picked}`;
  const [window, setWindow] = useState({ token, count: 20, height: 0 });
  const count = Math.min(memories.length, Math.max(window.token === token ? window.count : 20, selected + 1));
  const sentinel = useRef<HTMLDivElement>(null), first = useRef<HTMLDivElement>(null), active = useRef<HTMLDivElement>(null), edit = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => { if (window.token !== token) setWindow({ token, count: 20, height: 0 }); }, [token, window.token]);
  useLayoutEffect(() => { active.current?.scrollIntoView({ block: "nearest" }); }, [selected]);
  useLayoutEffect(() => { if (mode === "edit" && edit.current) { edit.current.focus(); edit.current.setSelectionRange(edit.current.value.length, edit.current.value.length); } }, [mode, selected]);
  useLayoutEffect(() => {
    const end = sentinel.current;
    if (!end || typeof IntersectionObserver === "undefined") return;
    const root = end.closest("#mlist");
    const observer = new IntersectionObserver(entries => {
      if (!entries.some(entry => entry.isIntersecting)) return;
      const height = window.height || (first.current && count ? Math.max(0, (end.getBoundingClientRect().top - first.current.getBoundingClientRect().top) / count) : 0);
      setWindow({ token, count: Math.min(memories.length, count + 20), height });
    }, { root, rootMargin: "800px 0px" });
    observer.observe(end);
    return () => observer.disconnect();
  }, [token, count, memories.length, window.height]);
  const limit = typeof IntersectionObserver === "undefined" ? memories.length : count;
  return <>{memories.slice(0, limit).map((memory, index) => {
    const on = index === selected, cat = memory.category || "Other", scope = scopes.get(memory.id);
    const meta = [MEM_SOURCE[memory.source] ?? "", shortDate(memory.updated)].filter(Boolean).join(" · ") + (memory.valid_from || memory.valid_until ? ` · ${memory.valid_from ? shortDate(memory.valid_from) : ""} to ${memory.valid_until ? shortDate(memory.valid_until) : ""}` : "");
    return <Fragment key={memory.id}>{(!index || cat !== (memories[index - 1].category || "Other")) && <div className="dsec bare">{cat}</div>}
      <div ref={element => { if (!index) first.current = element; if (on) active.current = element; }} className={`dmem${memory.valid_until && memory.valid_until < today && !(on && mode === "edit") ? " ended" : ""}`} role={on && mode === "edit" ? undefined : "option"} aria-selected={on} data-i={index}>
        <div className="top">{on && mode === "edit" ? <><textarea ref={edit} id="medit" rows={2} aria-label="Memory" defaultValue={memory.content} /><span className="dfine dsave"><span dangerouslySetInnerHTML={{ __html: ICON.ret }} /> save</span></> : <><span className="c" dangerouslySetInnerHTML={{ __html: mark(memory.content, query) }} />{on && <span className="acts"><button className="dbtn" data-m="edit">Edit</button><button className="dbtn warn" data-m="forget">Forget<kbd>⌘⌫</kbd></button></span>}</>}</div>
        <div className="m">{scope && <><span dangerouslySetInnerHTML={{ __html: scopeTag(scope) }} /> · </>}{meta}</div>{!(on && mode === "edit") && memory.quote && <q dangerouslySetInnerHTML={{ __html: mark(memory.quote, query) }} />}
      </div></Fragment>;
  })}{limit < memories.length && <div ref={sentinel} style={{ height: (memories.length - limit) * window.height }} />}</>;
}
