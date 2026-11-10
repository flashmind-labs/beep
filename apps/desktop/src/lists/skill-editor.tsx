import { HintView, type HintViewModel } from "../bar/hints-view.tsx";
import { useLayoutEffect, useRef } from "react";
import type { Skill } from "./skills.ts";
import { ICON } from "../ui/icons.tsx";

export function SkillEditor({ skill, error, hints, save, cancel }: { skill: Skill; error: string; hints: HintViewModel; save(): void; cancel(): void }) {
  const body = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => { body.current?.focus(); }, []);
  return <><div className="dhead"><span className="tt"><b>Edit skill</b><small>Beep keeps the old version, so you can undo</small></span></div>
    {error && <div className="derr dpadx">{error}</div>}
    <div className="dform dscroll">
      <label className="dfield"><span>Name</span><input id="kname" maxLength={60} defaultValue={skill.name} spellCheck={false} /></label>
      <label className="dfield"><span>When it applies</span><input id="kdesc" maxLength={200} defaultValue={skill.description} /></label>
      <label className="dfield"><span>Instructions</span><textarea ref={body} id="kbody" rows={12} defaultValue={skill.body} /></label>
      <div className="dline"><button type="button" className="dbtn pri" data-k="save" onClick={event => { event.stopPropagation(); save(); }}>Save<kbd dangerouslySetInnerHTML={{ __html: `⌘${ICON.ret}` }} /></button><button type="button" className="dbtn quiet" data-k="view" onClick={event => { event.stopPropagation(); cancel(); }}>Cancel</button></div>
    </div><HintView value={hints} /></>;
}
