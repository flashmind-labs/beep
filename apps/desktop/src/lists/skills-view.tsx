import { HintView, type HintViewModel } from "../bar/hints-view.tsx";
import { Fragment, useLayoutEffect, useRef } from "react";
import { ICON } from "../ui/icons.ts";
import { SKILL_ICON, skillMeta, type Skill } from "./skills.ts";

type Mode = "list" | "actions" | "delete" | "view" | "vdelete";
function Markup({ html }: { html: string }) { return <span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: html }} />; }
function Toggle({ on, label, index }: { on: boolean; label: string; index?: number }) {
  return <button className="dswitch" role="switch" aria-checked={on} aria-label={label} data-k="toggle" data-i={index} />;
}
function Key({ html }: { html: string }) { return <kbd dangerouslySetInnerHTML={{ __html: html }} />; }

export function SkillsList({ list, selected, mode, loaded, error, summary, hints }: { list: Skill[]; selected: number; mode: Mode; loaded: boolean; error: string; summary: string; hints: HintViewModel }) {
  const chosen = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => { chosen.current?.scrollIntoView({ block: "nearest" }); }, [selected]);
  const groups: { label: string; rows: { skill: Skill; index: number }[] }[] = [];
  list.forEach((skill, index) => {
    if (!index || skill.group_id !== list[index - 1].group_id) groups.push({ label: skill.group_name || (skill.group_id ? "Group" : "Yours"), rows: [] });
    groups[groups.length - 1].rows.push({ skill, index });
  });
  return <><div className="dhead"><span className="tt"><b>Skills</b><small>{summary}</small></span></div>
    {error && <div className="derr dpadx">{error}</div>}
    {loaded && !list.length ? <div className="dempty"><span className="dicon"><Markup html={SKILL_ICON} /></span><b className="dbig">No skills yet.</b><span className="dmuted dblock">Beep saves how you like things done here.</span></div> :
      <div className="dlist top" id="klist" role="listbox" aria-label="Skills">{groups.map((group, groupIndex) => <Fragment key={groupIndex}><div className="dsec flat dpad">{group.label}</div><div className="dlist dflush">{group.rows.map(({ skill, index }) => {
        const active = index === selected;
        if (active && mode === "delete") return <div className="drt" aria-selected="true" data-i={index} key={skill.id} ref={chosen}><span className="tt"><b>Delete <b>{skill.name}</b>?</b></span><span className="acts"><button className="dbtn warn" data-k="delete-yes">Delete</button><button className="dbtn quiet" data-k="list">Cancel</button></span></div>;
        return <div className={`drt${skill.enabled ? "" : " off"}`} role="option" aria-selected={active} data-i={index} key={skill.id} ref={active ? chosen : null}><span className="ic"><Markup html={SKILL_ICON} /></span><span className="tt"><b>{skill.name}</b><span>{skill.description}</span><small>{skillMeta(skill)}</small></span>
          {active && mode === "actions" ? <span className="acts"><button className="dbtn" data-k="view">Open<Key html={ICON.ret} /></button>{skill.can_undo && <button className="dbtn" data-k="undo">Undo<kbd>⌘Z</kbd></button>}<button className="dbtn warn" data-k="delete">Delete<kbd>⌘⌫</kbd></button></span> : <Toggle on={skill.enabled} label={`Use ${skill.name}`} index={index} />}</div>;
      })}</div></Fragment>)}</div>}
    <HintView value={hints} />
  </>;
}

export function SkillDetail({ skill, mode, error, hints }: { skill: Skill; mode: Mode; error: string; hints: HintViewModel }) {
  return <><div className="dhead"><span className="tt"><b>{skill.name}</b><small>{skill.group_name ? `${skill.group_name} · ${skill.description}` : skill.description}</small></span><Toggle on={skill.enabled} label="Use this skill" /><button className="dbtn" data-k="edit">Edit<Key html={`⌘${ICON.ret}`} /></button><button className="dbtn warn" data-k="vdelete">Delete<kbd>⌘⌫</kbd></button></div>
    {error && <div className="derr dpadx">{error}</div>}
    {mode === "vdelete" && <div className="dcard dcardm"><div className="dask"><b className="dtitle">Delete this skill?</b><div className="dmuted dblock">Beep forgets how you like "{skill.name}" done. It may learn it again if you keep asking for it.</div><div className="dline"><button className="dbtn warn" data-k="delete-yes">Delete<Key html={ICON.ret} /></button><button className="dbtn quiet" data-k="view">Cancel</button></div></div></div>}
    <div className="dbody pt"><div className="dmuted">{skillMeta(skill)}</div><div className="reply" dangerouslySetInnerHTML={{ __html: skill.html }} />{skill.can_undo && <div className="dmuted dtop dpt"><span className="dfill">{skill.edited ? "You changed this last." : "Beep changed this last."}</span><button className="dbtn" data-k="undo">Undo<kbd>⌘Z</kbd></button></div>}</div><HintView value={hints} />
  </>;
}
