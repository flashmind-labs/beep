import { useMemo, useState, type CSSProperties } from "react";
import { addMention } from "../agents/workdrop.ts";
import { invoke } from "../ui/tauri.ts";
import { highlight, languageOf, MAX_LINES, shortSize } from "./files-state.ts";
import { isHtmlFile } from "./html-preview-state.ts";
import { type MediaTab, type TreeText, P, saveWorkFile } from "./media.ts";
import { readError } from "./read.ts";
import { mentioned } from "./worktree-state.ts";

export function TextView({ tab, text }: { tab: MediaTab; text: TreeText }) {
  const [, update] = useState(0);
  const [note, setNote] = useState<string | null>(null);
  const rel = tab.rel ?? "", lang = languageOf(rel);
  const { about, style, code } = useMemo(() => {
    const all = text.binary ? [] : text.text.split("\n");
    if (all.length > 1 && all.at(-1) === "") all.pop();
    const lines = all.length, shown = all.slice(0, MAX_LINES);
    const cut = text.cut || lines > MAX_LINES ? ` · the first ${lines > MAX_LINES ? `${MAX_LINES.toLocaleString()} lines` : shortSize(text.text.length)} shown` : "";
    const about = [rel, lang, text.binary ? "" : `${lines.toLocaleString()} lines`, shortSize(text.size)].filter(Boolean).join(" · ") + cut;
    const style: CSSProperties & Record<"--lnw", string> = { "--lnw": `${String(shown.length).length}ch` };
    const code = shown.map((line, index) => `<span class="ln">${index + 1}</span>${highlight(line, lang)}`).join("\n");
    return { about, style, code };
  }, [text, rel, lang]);
  const refresh = () => { if (P.tabs.includes(tab)) update(value => value + 1); };
  const togglePreview = async () => {
    const session = P.session;
    if (!session) return;
    const preview = tab.preview ??= { active: false, loading: false, src: "", error: "" };
    preview.active = !preview.active;
    if (!preview.active || preview.src || preview.loading) { refresh(); return; }
    preview.loading = true; preview.error = ""; refresh();
    try {
      const result = await invoke<{ kind: string; src: string }>("agent_artifact", { machineId: session.machine_id, root: tab.root ?? session.cwd, path: rel, preview: true });
      if (result.kind !== "html") throw new Error("This file cannot be previewed as HTML.");
      preview.src = result.src;
    } catch (error) {
      preview.error = readError(error);
    } finally {
      preview.loading = false; refresh();
    }
  };
  const preview = tab.preview;
  return <>{preview?.active ? preview.loading ? <div className="agpmsg">Loading preview…</div>
    : preview.error ? <div className="agpmsg derr">{preview.error}</div>
    : <div className="agartframe-box"><iframe className="agartframe" sandbox="allow-scripts" referrerPolicy="no-referrer" title={`Preview of ${tab.name}`} src={preview.src || undefined} /></div>
    : text.binary ? <div className="agpmsg">{tab.name} is not text ({shortSize(text.size)}). Save it to open it on this computer.</div>
    : <div className="agcode" style={style}><pre dangerouslySetInnerHTML={{ __html: code }} /></div>}
    <div className="agpacts"><span className="meta mono" title={rel}>{note ?? about}</span><span className="sp" />
      {!text.binary && isHtmlFile(rel) && <button className="dbtn" data-preview="1" onClick={() => void togglePreview()}>{preview?.active ? "Source" : "Preview"}</button>}
      <button className="dbtn" data-tment="1" onClick={() => addMention(mentioned(tab.root ?? P.session?.cwd ?? "", P.session?.cwd ?? "", rel))}>Mention</button>
      <button className="dbtn" data-tsave="1" onClick={() => saveWorkFile(rel, setNote, tab.root)}>Save</button>
    </div></>;
}
