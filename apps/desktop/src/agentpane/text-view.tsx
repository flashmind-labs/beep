import { useLayoutEffect, useMemo, useRef, type CSSProperties } from "react";
import { addMention } from "../agents/workdrop.ts";
import { invoke } from "../ui/tauri.ts";
import { highlight, languageOf, MAX_LINES, shortSize } from "./files-state.ts";
import { isHtmlFile } from "./html-preview-state.ts";
import { type MediaTab, type TreeText, P, saveWorkFile } from "./media.ts";
import { noteTab, touchTab } from "./media-draw.ts";
import { fileError } from "./read.ts";
import { readView, viewKey, writeView } from "./view-memory.ts";
import { mentioned } from "./worktree-state.ts";
import { Button } from "../ui/kit.tsx";

/// Shows an HTML file's preview in its tab, reading it the first time, or its source again.
export async function togglePreview(tab: MediaTab) {
  const session = P.session;
  if (!session || !tab.rel) return;
  const preview = tab.preview ??= { active: false, loading: false, src: "", error: "" };
  preview.active = !preview.active;
  if (!preview.active || preview.src || preview.loading) { touchTab(tab); return; }
  preview.loading = true; preview.error = ""; touchTab(tab);

  try {
    const result = await invoke<{ kind: string; src: string }>("agent_artifact", { machineId: session.machine_id, root: tab.root ?? session.cwd, path: tab.rel, preview: true });
    if (result.kind !== "html") throw new Error("This file cannot be previewed as HTML.");
    preview.src = result.src;
  } catch (error) {
    preview.error = fileError(error, tab.rel);
  } finally {
    preview.loading = false; touchTab(tab);
  }
}

/// Where the code of a file's tab was scrolled to, kept with the session's views.
interface Scrolled { top: number; left: number }

const readScrolled = (key: string): Scrolled | null => {
  const saved = readView(key);
  return saved && typeof saved === "object" && "top" in saved && "left" in saved && typeof saved.top === "number" && typeof saved.left === "number" ? { top: saved.top, left: saved.left } : null;
};

export function TextView({ tab, text }: { tab: MediaTab; text: TreeText }) {
  const code = useRef<HTMLDivElement>(null);
  const rel = tab.rel ?? "", lang = languageOf(rel);
  const scrollKey = P.session ? `${viewKey(P.session.machine_id, P.session.id)}:file:${tab.id}` : null;
  const { about, style, html } = useMemo(() => {
    const all = text.binary ? [] : text.text.split("\n");
    if (all.length > 1 && all.at(-1) === "") all.pop();
    const lines = all.length, shown = all.slice(0, MAX_LINES);
    const cut = text.cut || lines > MAX_LINES ? ` · the first ${lines > MAX_LINES ? `${MAX_LINES.toLocaleString()} lines` : shortSize(text.text.length)} shown` : "";
    const about = [rel, lang, text.binary ? "" : `${lines.toLocaleString()} lines`, shortSize(text.size)].filter(Boolean).join(" · ") + cut;
    const style: CSSProperties & Record<"--lnw", string> = { "--lnw": `${String(shown.length).length}ch` };
    const html = shown.map((line, index) => `<span class="ln${index + 1 === tab.line ? " on" : ""}">${index + 1}</span>${highlight(line, lang)}`).join("\n");
    return { about, style, html };
  }, [text, rel, lang, tab.line]);
  const preview = tab.preview;
  const source = !preview?.active && !text.binary;

  // The code scrolls back to where it was the last time the file showed.
  useLayoutEffect(() => {
    const saved = scrollKey && source ? readScrolled(scrollKey) : null;
    if (!saved) return;
    const frame = requestAnimationFrame(() => { if (code.current) { code.current.scrollTop = saved.top; code.current.scrollLeft = saved.left; } });
    return () => cancelAnimationFrame(frame);
  }, [scrollKey, source]);

  // A link to a line shows that line.
  useLayoutEffect(() => {
    if (!source || !tab.line) return;
    const frame = requestAnimationFrame(() => code.current?.querySelector(".ln.on")?.scrollIntoView({ block: "center" }));
    return () => cancelAnimationFrame(frame);
  }, [source, tab.line, html]);

  const scrolled = () => { if (scrollKey && code.current) writeView(scrollKey, { top: code.current.scrollTop, left: code.current.scrollLeft }); };
  return <>{preview?.active ? preview.loading ? <div className="agpmsg">Loading preview…</div>
    : preview.error ? <div className="agpmsg derr">{preview.error}</div>
    : <div className="agartframe-box"><iframe className="agartframe" sandbox="allow-scripts" referrerPolicy="no-referrer" title={`Preview of ${tab.name}`} src={preview.src || undefined} /></div>
    : text.binary ? <div className="agpmsg">{tab.name} is not text ({shortSize(text.size)}). Save it to open it on this computer.</div>
    : <div className="agcode" style={style} ref={code} onScroll={scrolled}><pre dangerouslySetInnerHTML={{ __html: html }} /></div>}
    <div className="agpacts"><span className="meta mono" title={rel}>{tab.note ?? about}</span><span className="sp" />
      {!text.binary && isHtmlFile(rel) && <Button data-preview="1" onClick={() => void togglePreview(tab)}>{preview?.active ? "Source" : "Preview"}</Button>}
      <Button data-tment="1" onClick={() => addMention(mentioned(tab.root ?? P.session?.cwd ?? "", P.session?.cwd ?? "", rel))}>Mention</Button>
      <Button data-tsave="1" onClick={() => saveWorkFile(rel, text => noteTab(tab, text), tab.root)}>Save</Button>
    </div></>;
}
