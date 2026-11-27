import { Button, NO_AUTO } from "../../ui/kit.tsx";
import { Fragment, useLayoutEffect, useRef, type ReactNode, type RefObject } from "react";
import type { ProjectWindowProps } from "../../stores/project/projectwindow.ts";
import type { Hit } from "./palette.tsx";
import { useDialogRef } from "../../ui/parts.tsx";

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

const DEFAULT_PLACEHOLDER = "Search anything: projects, coding agents, machines, settings, PRs…";

export function PaletteView({ input, hits, selected, query, placeholder = DEFAULT_PLACEHOLDER, searchIcon, keys, group, icon, search, choose, hover, windowProps }: {
  input: RefObject<HTMLInputElement | null>; hits: Hit[]; selected: number; query: string;
  placeholder?: string;
  searchIcon: string;
  /// The footer's keys, each with what it does.
  keys: string[][];
  group(hit: Hit): string; icon(hit: Hit): ReactNode;
  windowProps(hit: Hit): ProjectWindowProps | null;
  search(value: string): void; choose(hit: Hit): void; hover(index: number): void;
}) {
  const active = useRef<HTMLButtonElement>(null), dialog = useDialogRef();
  useLayoutEffect(() => { input.current?.focus(); }, []);
  useLayoutEffect(() => { active.current?.scrollIntoView({ block: "nearest" }); }, [selected, query]);
  return <div ref={dialog} className="cc-pal" role="dialog" aria-label="Search"><div className="q"><span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: searchIcon }} /><input ref={input} type="text" placeholder={placeholder} {...NO_AUTO} aria-label="Search" role="combobox" aria-controls="pal-list" aria-expanded="true" aria-activedescendant={hits[selected] ? `pal-opt-${selected}` : undefined} onChange={e => search(e.currentTarget.value)} /></div>
    <div className="res" role="listbox" id="pal-list">{hits.map((hit, i) => {
      const title = group(hit), previous = i ? group(hits[i - 1]) : null;
      const detail = hit.detail && hit.kind !== "action" && hit.kind !== "machine" ? hit.detail : null;
      const side = hit.detail && hit.kind === "machine" ? hit.detail : null;
      return <Fragment key={`${hit.kind}:${hit.id}`}>{title !== previous && <h5>{title}</h5>}<Button look="plain" {...windowProps(hit)?.clicks} {...windowProps(hit)?.menu} ref={i === selected ? active : null} type="button" className="it" tabIndex={-1} role="option" aria-selected={i === selected} id={`pal-opt-${i}`} onMouseMove={e => { if (e.movementX || e.movementY) hover(i); }} onClick={() => choose(hit)}>
        {hit.kind === "machine" ? <span className={`ddot${hit.detail === "online" ? "" : " off"}`} /> : icon(hit)}
        <span className="t"><span><Marked text={hit.title} query={query} /></span>{detail && <small><Marked text={detail} query={query} /></small>}{hit.snippet && <small className="s"><Marked text={hit.snippet} query={query} /></small>}</span>{side && <span className="w">{side}</span>}
      </Button></Fragment>;
    })}{!hits.length && <div className="none">Nothing matches “{query}”.</div>}</div>
    <div className="foot">{keys.map(([key, label]) => <span key={key}><kbd>{key}</kbd>{label}</span>)}</div>
  </div>;
}
