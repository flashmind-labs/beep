import { Fragment, useLayoutEffect, useMemo, useRef } from "react";
import { patchHtml } from "../ui/patch.ts";
import { answerHtml } from "../agents/turnrows.ts";
import { ICON } from "../ui/icons.tsx";
import { agentOf, count, progress, RUNNING, type Open, type Source } from "./research-state.ts";
import { esc } from "../ui/format.ts";

/// Citation numbers as links to their sources, which open in the browser.
function sup(sources: readonly Source[], numbers: number[]) {
  const links = numbers.map(n => {
    const s = sources.find(s => s.number === n);
    return s ? `<a class="rcite" href="${esc(s.url)}" title="${esc(s.title)}">${n}</a>` : String(n);
  });
  return `<sup>${links.join(", ")}</sup>`;
}

/// The rendered paper with "[3]" and "[1, 4]" turned into citation marks,
/// outside links and code. The paper is sanitized HTML; this only adds links.
function cite(sources: readonly Source[], html: string) {
  const doc = new DOMParser().parseFromString(`<body>${html}</body>`, "text/html");
  const walk = doc.createTreeWalker(doc.body, NodeFilter.SHOW_TEXT);
  const texts: Text[] = [];
  while (walk.nextNode()) if (walk.currentNode instanceof Text && !walk.currentNode.parentElement?.closest("a, code, pre")) texts.push(walk.currentNode);

  for (const node of texts) {
    if (!/\[\d+(?:\s*,\s*\d+)*\]/.test(node.data)) continue;
    const marked = new DOMParser().parseFromString(`<body>${esc(node.data).replace(/\[(\d+(?:\s*,\s*\d+)*)\]/g, (_, list: string) => sup(sources, list.split(",").map(Number)))}</body>`, "text/html");
    node.replaceWith(...marked.body.childNodes);
  }

  return doc.body.innerHTML;
}

function RichText({ html }: { html: string }) {
  const node = useRef<HTMLSpanElement>(null);
  useLayoutEffect(() => { if (node.current) patchHtml(node.current, html); }, [html]);
  return <span ref={node} style={{ display: "contents" }} />;
}
function Answer({ html }: { html: string }) {
  const node = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => { if (node.current) patchHtml(node.current, answerHtml(html)); }, [html]);
  return <div ref={node} className="dcoderen agent md" />;
}
function ExportButton({ exporting }: { exporting: boolean }) {
  return exporting ? <button className="dbtn" disabled><span className="dspin" />Exporting</button> : <button className="dbtn" data-act="export">Export PDF</button>;
}
export function ResearchChat({ research: o }: { research: Open }) {
  const status = o.research?.status, last = o.turns.findLastIndex(t => t.role === "assistant");
  return <>{o.turns.map((turn, i) => turn.role === "user" ? <div className="dcoderen user" key={i}><b>You</b>{turn.text && <span>{turn.text}</span>}</div> : turn.role === "scheduled" ? <div className="dmuted" key={i}>{turn.label || "Research finished"}</div> : turn.role === "assistant" && turn.html ? <Fragment key={i}><Answer html={turn.html} />{i === last && status === "planning" && o.sending === null && <div className="racts"><button className="dbtn pri" data-act="go">Start<kbd>⌘<span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: ICON.ret }} /></kbd></button></div>}{i === last && status === "done" && o.sending === null && <div className="racts"><button className="dbtn" data-act="tab:draft">Open the paper</button><ExportButton exporting={o.exporting} /></div>}</Fragment> : null)}
    {o.sending !== null && <div className="dcoderen agent md"><RichText html={o.sending} /><span className="caret" /></div>}{RUNNING.has(status) && <div className="rstatus"><span className="dspin" />{progress(o, Date.now())}</div>}{o.error && <div className="derr">{o.error}</div>}</>;
}
export function ResearchPaper({ research: o }: { research: Open }) {
  const r = o.research, status = r?.status ?? "planning";
  const html = useMemo(() => r ? cite(r.sources, r.draft_html) : "", [r?.sources, r?.draft_html]);
  if (!r || (status === "planning" && !o.entries.some(e => e.kind === "note"))) return <div className="rpane"><div className="rbox rempty">The workspace fills in once the team starts.</div></div>;
  if (!r.draft) return <div className="rpane"><div className="rbox rempty">{RUNNING.has(status) ? "The writer starts after the critic's review." : "No paper was written."}</div></div>;
  return <><div className="rpane"><div className="rdoc"><span className="by">Written by {agentOf(o, "writer").name}, reviewed by {agentOf(o, "critic").name}. {r.sources.length} sources.</span><RichText html={html} /></div></div><div className="agpacts"><span className="meta">Markdown, {count(r.sources.length, "source")}</span><span className="sp" /><button className="dbtn" data-act="copy">Copy</button><ExportButton exporting={o.exporting} /></div></>;
}
