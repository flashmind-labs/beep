import { HintView, type HintViewModel } from "../bar/hints-view.tsx";
import { Fragment, useLayoutEffect, useRef } from "react";
import { ICON, Icon, type IconName } from "../ui/icons.tsx";
import { AppLogo } from "../chat/applogo-view.tsx";
import { describeDay, routineWhen, span, type Cron, type Grant } from "./routines.ts";

function Glyph({ name, size }: { name: IconName; size?: number }) { return <span style={{ display: "contents" }}><Icon name={name} size={size} /></span>; }
function useSelected(index: number) {
  const ref = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => { ref.current?.scrollIntoView({ block: "nearest" }); }, [index]);
  return ref;
}
export function RoutinesList(p: { list: Cron[]; selected: number; mode: "list" | "actions" | "delete"; loaded: boolean; error: string; summary: string; grants: number; hints: HintViewModel }) {
  const selected = useSelected(p.selected);
  return <><div className="dhead"><span className="tt"><b>Routines</b><small>{p.summary}</small></span><button className="dbtn" data-r="grants">Grants{p.grants > 0 && <> <span className="dmut">{p.grants}</span></>}<kbd>⌘G</kbd></button><button className="dbtn" data-r="new"><Glyph name="add" />New<kbd>⌘N</kbd></button></div>
    {p.error && <div className="derr dpadx">{p.error}</div>}
    {p.loaded && !p.list.length ? <div className="dempty"><span className="dicon"><Glyph name="rout" size={16} /></span><b className="dbig">No routines yet.</b><span className="dmuted dblock">Schedule tasks for Beep to run on a timer.</span><button className="dbtn pri" data-r="new">New routine</button></div> : <div className="dlist top" id="rlist" role="listbox" aria-label="Routines">{p.list.map((routine, index) => {
      const on = index === p.selected, label = routine.label || "Reminder";
      if (on && p.mode === "delete") return <div key={routine.id} ref={selected} className="drt" aria-selected="true" data-i={index}><span className="tt"><b>Delete <b>{label}</b>?</b></span><span className="acts"><button className="dbtn warn" data-r="delete-yes">Delete</button><button className="dbtn quiet" data-r="list">Cancel</button></span></div>;
      const next = routine.enabled ? routine.next_run_at && !routine.run_once ? `Next: ${describeDay(new Date(routine.next_run_at))}` : "" : "Paused";
      return <div key={routine.id} ref={on ? selected : null} className={`drt${routine.enabled ? "" : " off"}`} role="option" aria-selected={on} data-i={index}><span className="ic"><Glyph name="rout" size={16} /></span><span className="tt"><b>{label}</b><span>{routine.prompt}</span><small>{[routineWhen(routine.schedule), next].filter(Boolean).join(" · ")}</small></span>{on && p.mode === "actions" ? <span className="acts"><button className="dbtn" data-r="run"><Glyph name="playSmall" />Run now</button><button className="dbtn" data-r="edit">Edit<kbd>⌘E</kbd></button><button className="dbtn warn" data-r="delete">Delete<kbd>⌘⌫</kbd></button></span> : <button className="dswitch" role="switch" aria-checked={routine.enabled} aria-label={`${label} on`} data-r="toggle" data-i={index} />}</div>;
    })}</div>}
    <HintView value={p.hints} /></>;
}
export function GrantsList(p: { list: Grant[]; selected: number; mode: "list" | "confirm"; loaded: boolean; error: string; hints: HintViewModel }) {
  const selected = useSelected(p.selected), now = Date.now();
  return <><div className="dhead"><span className="tt"><b>Routine grants</b><small>{p.loaded && !p.list.length ? "Nothing granted" : "What routines may do without asking"}</small></span></div>
    {p.error && <div className="derr dpadx">{p.error}</div>}
    {p.loaded && !p.list.length ? <div className="dempty"><span className="dicon"><Glyph name="shield" /></span><b className="dbig">No routine grants.</b><span className="dmuted dblock">Beep asks in a chat when a routine needs one.</span></div> : <div className="dlist top" id="glist" role="listbox" aria-label="Routine grants">{p.list.map((grant, index) => {
      const on = index === p.selected, action = grant.action.replace(/ for a routine$/, "");
      if (on && p.mode === "confirm") return <div key={grant.id} ref={selected} className="drt" aria-selected="true" data-i={index}><span className="tt"><b>Revoke? <b>{grant.routine}</b> will ask before each {action} again.</b></span><span className="acts"><button className="dbtn warn" data-g="revoke-yes">Revoke</button><button className="dbtn quiet" data-g="list">Cancel</button></span></div>;
      const per = grant.per_run === 1 ? "1 each run" : `up to ${grant.per_run} each run`, ends = grant.expires_at > now ? `Ends in ${span(grant.expires_at - now)}` : "Ended", used = grant.last_used_at ? `Last used ${span(now - grant.last_used_at)} ago` : "Not used yet";
      return <div key={grant.id} ref={on ? selected : null} className="drt dalignt" role="option" aria-selected={on} data-i={index}><AppLogo app={grant.app} style={{ marginTop: "2px" }} /><span className="tt"><b>{grant.routine}</b><span>{grant.app}: {action}, {per}</span><small>{ends} · {used}</small>{on && !!grant.fields.length && <span className="dgkv">{grant.fields.map((field, i) => <Fragment key={i}><span>{field.label}</span><span>{field.value}</span></Fragment>)}</span>}</span>{on && <span className="acts"><button className="dbtn warn" data-g="revoke">Revoke<kbd>⌘⌫</kbd></button></span>}</div>;
    })}</div>}
    <HintView value={p.hints} /></>;
}

export function RoutineEditor(p: { form: import("./routines.ts").RoutineForm; hints: HintViewModel; input(key: "label" | "prompt" | "time" | "date" | "cron", value: string): void }) {
  const { form } = p, edit = form.edit, missing = form.error === "prompt";
  const input = (key: "label" | "prompt" | "time" | "date" | "cron") => (event: React.FormEvent<HTMLInputElement | HTMLTextAreaElement>) => p.input(key, event.currentTarget.value);
  const choices: [import("./routines.ts").Repeat, string][] = [["day", "Every day"], ["weekdays", "Weekdays"], ["week", "Weekly"], ["hour", "Every hour"], ["once", "Once"], ["cron", "Cron"]];
  const time = <label className={`dfield${form.repeat === "once" ? "" : " dnarrow"}`}><span>{form.repeat === "hour" ? "Minute past" : "Time"}</span><input id="nrtime" type="time" defaultValue={form.time} onInput={input("time")} /></label>;
  return <><div className="dhead"><span className="tt"><b>{edit ? "Edit routine" : "New routine"}</b><small>{edit ? edit.label || "Reminder" : "Runs on this account, phone or computer off"}</small></span></div><div className="dform dscroll">
    <label className="dfield"><span>Name</span><input id="nrlabel" placeholder="Morning briefing" defaultValue={form.label} onInput={input("label")} /></label>
    {edit ? <div className="dfield"><span>What Beep does</span><div className="dtext">{form.prompt}</div></div> : <label className={`dfield${missing ? " err" : ""}`}><span>{missing ? "Tell Beep what to do" : "What should Beep do?"}</span><textarea id="nrprompt" rows={2} placeholder="Summarise my calendar and the weather" defaultValue={form.prompt} onInput={input("prompt")} /></label>}
    {form.repeat === "place" && edit ? <div className="dfield"><span>Starts</span><div className="dtext">{routineWhen(edit.schedule)}</div><span>Change the place in a chat.</span></div> : <><div className="dfield"><span>Repeat</span><div className="dchips">{choices.map(([value, label]) => <button key={value} className="dchip" aria-pressed={form.repeat === value} data-rep={value}>{label}</button>)}</div></div>
      {form.repeat === "cron" ? <><label className="dfield"><span>Cron expression</span><input id="nrcron" className="mono plain" placeholder="30 7 * * 1-5" defaultValue={form.cron} spellCheck={false} onInput={input("cron")} /></label><span className="dmuted dpull" id="nrcronsays" /></> : form.repeat === "once" ? <div className="dtwo"><label className="dfield"><span>Date</span><input id="nrdate" type="date" defaultValue={form.date} onInput={input("date")} /></label>{time}</div> : <>{form.repeat === "week" && <div className="dchips">{[1, 2, 3, 4, 5, 6, 0].map(day => <button key={day} className="dchip day" aria-pressed={form.days.includes(day)} data-day={day}>{["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][day]}</button>)}</div>}{time}</>}</>}
    {form.error && !missing && <div className="derr">{form.error}</div>}
    <div className="dline"><button className="dbtn pri" id="nrgo" disabled={form.busy}>{edit ? "Save" : "Create"}<kbd dangerouslySetInnerHTML={{ __html: `⌘${ICON.ret}` }} /></button><button className="dbtn quiet" id="nrno">Cancel</button><span className="dmuted dpush">{edit ? "To change what it does, ask Beep in a chat" : "Or just tell Beep in a chat"}</span></div>
    </div><HintView value={p.hints} /></>;
}
