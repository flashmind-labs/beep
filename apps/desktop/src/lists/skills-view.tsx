import { HintView, type HintViewModel } from "../bar/hints-view.tsx";
import { Fragment, useLayoutEffect, useRef } from "react";
import { ICON, Icon } from "../ui/icons.tsx";
import { skillMeta, type Skill, type SkillAction } from "./skills.ts";

type Mode = "list" | "actions" | "delete" | "view" | "vdelete";
function SkillIcon() { return <span style={{ display: "contents" }}><Icon name="skill" /></span>; }
/// Runs a skill action on a row, or on the open skill with a null row, without the row's own click.
type Act = (action: SkillAction, index: number | null) => void;
const press = (act: Act, action: SkillAction, index: number | null = null) => ({ onClick: (event: React.MouseEvent) => { event.stopPropagation(); act(action, index); } });
function Toggle({ on, label, index, act }: { on: boolean; label: string; index: number | null; act: Act }) {
  return <button className="dswitch" role="switch" aria-checked={on} aria-label={label} {...press(act, "toggle", index)} />;
}
function Key({ html }: { html: string }) { return <kbd dangerouslySetInnerHTML={{ __html: html }} />; }

export function SkillsList({ list, selected, mode, loaded, error, summary, hints, act, pick, menu }: { list: Skill[]; selected: number; mode: Mode; loaded: boolean; error: string; summary: string; hints: HintViewModel;
  act: Act; pick(index: number): void; menu(event: MouseEvent, index: number): void }) {
  const chosen = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => { chosen.current?.scrollIntoView({ block: "nearest" }); }, [selected]);
  const groups: { label: string; rows: { skill: Skill; index: number }[] }[] = [];
  list.forEach((skill, index) => {
    if (!index || skill.group_id !== list[index - 1].group_id) groups.push({ label: skill.group_name || (skill.group_id ? "Group" : "Yours"), rows: [] });
    groups[groups.length - 1].rows.push({ skill, index });
  });
  return <><div className="dhead"><span className="tt"><b>Skills</b><small>{summary}</small></span></div>
    {error && <div className="derr dpadx">{error}</div>}
    {loaded && !list.length ? <div className="dempty"><span className="dicon"><SkillIcon /></span><b className="dbig">No skills yet.</b><span className="dmuted dblock">Beep saves how you like things done here.</span></div> :
      <div className="dlist top" id="klist" role="listbox" aria-label="Skills">{groups.map((group, groupIndex) => <Fragment key={groupIndex}><div className="dsec flat dpad">{group.label}</div><div className="dlist dflush">{group.rows.map(({ skill, index }) => {
        const active = index === selected;
        if (active && mode === "delete") return <div className="drt" aria-selected="true" data-i={index} key={skill.id} ref={chosen} onContextMenu={event => menu(event.nativeEvent, index)}><span className="tt"><b>Delete <b>{skill.name}</b>?</b></span><span className="acts"><button className="dbtn warn" {...press(act, "delete-yes", index)}>Delete</button><button className="dbtn quiet" {...press(act, "list", index)}>Cancel</button></span></div>;
        return <div className={`drt${skill.enabled ? "" : " off"}`} role="option" aria-selected={active} data-i={index} key={skill.id} ref={active ? chosen : null} onClick={() => pick(index)} onContextMenu={event => menu(event.nativeEvent, index)}><span className="ic"><SkillIcon /></span><span className="tt"><b>{skill.name}</b><span>{skill.description}</span><small>{skillMeta(skill)}</small></span>
          {active && mode === "actions" ? <span className="acts"><button className="dbtn" {...press(act, "view", index)}>Open<Key html={ICON.ret} /></button>{skill.can_undo && <button className="dbtn" {...press(act, "undo", index)}>Undo<kbd>⌘Z</kbd></button>}<button className="dbtn warn" {...press(act, "delete", index)}>Delete<kbd>⌘⌫</kbd></button></span> : <Toggle on={skill.enabled} label={`Use ${skill.name}`} index={index} act={act} />}</div>;
      })}</div></Fragment>)}</div>}
    <HintView value={hints} />
  </>;
}

export function SkillDetail({ skill, mode, error, hints, act }: { skill: Skill; mode: Mode; error: string; hints: HintViewModel; act: Act }) {
  return <><div className="dhead"><span className="tt"><b>{skill.name}</b><small>{skill.group_name ? `${skill.group_name} · ${skill.description}` : skill.description}</small></span><Toggle on={skill.enabled} label="Use this skill" index={null} act={act} /><button className="dbtn" {...press(act, "edit")}>Edit<Key html={`⌘${ICON.ret}`} /></button><button className="dbtn warn" {...press(act, "vdelete")}>Delete<kbd>⌘⌫</kbd></button></div>
    {error && <div className="derr dpadx">{error}</div>}
    {mode === "vdelete" && <div className="dcard dcardm"><div className="dask"><b className="dtitle">Delete this skill?</b><div className="dmuted dblock">Beep forgets how you like "{skill.name}" done. It may learn it again if you keep asking for it.</div><div className="dline"><button className="dbtn warn" {...press(act, "delete-yes")}>Delete<Key html={ICON.ret} /></button><button className="dbtn quiet" {...press(act, "view")}>Cancel</button></div></div></div>}
    <div className="dbody pt"><div className="dmuted">{skillMeta(skill)}</div><div className="reply" dangerouslySetInnerHTML={{ __html: skill.html }} />{skill.can_undo && <div className="dmuted dtop dpt"><span className="dfill">{skill.edited ? "You changed this last." : "Beep changed this last."}</span><button className="dbtn" {...press(act, "undo")}>Undo<kbd>⌘Z</kbd></button></div>}</div><HintView value={hints} />
  </>;
}
