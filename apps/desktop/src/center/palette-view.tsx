import { Fragment, useLayoutEffect, useRef } from "react";
import type { Hit } from "./palette.ts";

function Marked({ text, query }: { text: string; query: string }) {
  const terms = query.trim().split(/\s+/u).filter(Boolean);
  if (!terms.length) return text;
  const pattern = new RegExp(terms.map(term => term.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("|"), "giu");
  const parts: React.ReactNode[] = [];
  let end = 0;
  for (const match of text.matchAll(pattern)) {
    parts.push(text.slice(end, match.index), <mark key={match.index}>{match[0]}</mark>);
    end = match.index + match[0].length;
  }
  parts.push(text.slice(end));
  return <>{parts}</>;
}

export function PaletteView({ hits, selected, query, subscribed, searchIcon, group, icon, search, choose, hover, ask }: {
  hits: Hit[]; selected: number; query: string; subscribed: boolean;
  searchIcon: string;
  group(hit: Hit, query: string): string; icon(hit: Hit): string | null;
  search(value: string): void; choose(hit: Hit): void; hover(index: number): void; ask(value: string): void;
}) {
  const input = useRef<HTMLInputElement>(null), active = useRef<HTMLButtonElement>(null);
  useLayoutEffect(() => { input.current?.focus(); }, []);
  useLayoutEffect(() => { active.current?.scrollIntoView({ block: "nearest" }); }, [selected, query]);
  return <div className="cc-pal" role="dialog" aria-label="Search"><div className="q"><span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: searchIcon }} /><input ref={input} type="text" placeholder={`Search anything: ${subscribed ? "chats, " : ""}projects, coding agents, machines, settings, PRs…`} autoComplete="off" spellCheck={false} aria-label="Search" role="combobox" aria-controls="pal-list" aria-expanded="true" aria-activedescendant={hits[selected] ? `pal-opt-${selected}` : undefined} onChange={e => search(e.currentTarget.value)} /></div>
    <div className="res" role="listbox" id="pal-list">{hits.map((hit, i) => {
      const title = group(hit, query), previous = i ? group(hits[i - 1], query) : null;
      const detail = hit.detail && ["chat", "routine", "setting", "project", "project_chat", "folder", "agent_session", "code", "branch"].includes(hit.kind) ? hit.detail : null;
      const side = hit.detail && (hit.kind === "memory" || hit.kind === "machine") ? hit.detail : null;
      const multi = query.trim().split(/\s+/u).length > 1;
      return <Fragment key={`${hit.kind}:${hit.id}`}>{title !== previous && <h5>{title}</h5>}<button ref={i === selected ? active : null} type="button" className="it" tabIndex={-1} role="option" aria-selected={i === selected} id={`pal-opt-${i}`} onMouseMove={e => { if (e.movementX || e.movementY) hover(i); }} onClick={() => choose(hit)}>
        {hit.kind === "machine" ? <span className={`ddot${hit.detail === "online" ? "" : " off"}`} /> : <span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: icon(hit) ?? "" }} />}
        <span className="t"><span><Marked text={hit.title} query={query} /></span>{detail && <small style={multi ? { whiteSpace: "normal" } : undefined}><Marked text={detail} query={query} /></small>}{hit.snippet && <small style={multi ? { whiteSpace: "normal" } : undefined}><Marked text={hit.snippet} query={query} /></small>}</span>{side && <span className="w">{side}</span>}
      </button></Fragment>;
    })}{!hits.length && <div className="none">Nothing matches “{query}”.{query && subscribed && <><br /><button type="button" className="dbtn" onClick={() => ask(query)}>Ask Beep “{query}”<kbd>⌘↵</kbd></button></>}</div>}</div>
    <div className="foot">{[["↑↓", "move"], ["↵", "open"], ...(subscribed ? [["⌘↵", "ask Beep"]] : []), ["esc", "close"]].map(([key, label]) => <span key={key}><kbd>{key}</kbd>{label}</span>)}</div>
  </div>;
}
