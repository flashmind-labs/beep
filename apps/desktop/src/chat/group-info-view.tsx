import type { ComponentProps } from "react";
import { BEEP_MARK } from "../ui/icons.tsx";
import { HintView, type HintViewModel } from "../bar/hints-view.tsx";
import { GroupHeader } from "./group-shell-view.tsx";
import type { GroupMember, GroupReminder, GroupSummary } from "./groups.ts";

interface Props {
  group: GroupSummary | null; reminders: GroupReminder[] | null; leave: boolean; error: string; busy: ReadonlySet<string>;
  color(name: string): string; schedule(value: string): string; share(): void; remove(id: string): void; askLeave(): void; stay(): void; leaveGroup(): void;
}
function Avatar({ name, label, color }: { name: string; label: string; color(name: string): string }) {
  return name === "Beep" ? <span className="dav" style={{ background: color(name) }} dangerouslySetInnerHTML={{ __html: BEEP_MARK }} /> : <span className="dav" style={{ background: color(name) }}>{[...label][0]?.toUpperCase() ?? "?"}</span>;
}
function Member({ name, label, note, color }: { name: string; label: string; note: string; color(name: string): string }) { return <div className="member"><Avatar name={name} label={label} color={color} /><span><b>{name}</b><small>{note}</small></span></div>; }
const date = (ms: number) => new Date(ms).toLocaleDateString(undefined, { day: "numeric", month: "short" });
/// The group's info page in the bar: its header, the info and the keys.
export function GroupInfoPage({ header, info, hints }: { header: ComponentProps<typeof GroupHeader>; info: Props; hints: HintViewModel }) {
  return <><GroupHeader {...header} /><div className="dbody pt dgroupbody" id="ginfobody"><GroupInfoView {...info} /></div><HintView value={hints} /></>;
}

function GroupInfoView(p: Props) {
  const g = p.group;
  if (!g) return <div className="dmuted"><span className="dspin" />Loading…</div>;
  const members: GroupMember[] = [...g.members].sort((a, b) => Number(b.you) - Number(a.you));
  const own = g.members.find(member => member.you)?.name, ask = 'Ask Beep in the chat to add one, like "remind us at 10 to check out".';
  return <>{p.error && <div className="derr">{p.error}</div>}<div className="dsec flat">Members</div><div className="dinfo">{members.map(member => <Member key={`${member.name}:${member.joined_at}`} name={member.you ? "You" : member.name} label={member.name} note={member.you ? `${member.name}, ${member.creator ? "created the group" : `joined ${date(member.joined_at)}`}` : member.creator ? "Created the group" : `Joined ${date(member.joined_at)}`} color={p.color} />)}<Member name="Beep" label="Beep" note="Joins in when it can help, or when someone writes @beep" color={p.color} /></div>
    <div className="dsec flat">Reminders</div>{!p.reminders ? <div className="dmuted"><span className="dspin" />Loading…</div> : !p.reminders.length ? <div className="dinfo dmut">No reminders yet. {ask}</div> : <div className="dinfo">{p.reminders.map(reminder => <div className="member dalignt" key={reminder.id}><span className="dcol"><b>{reminder.label}</b><small>{reminder.message}</small><small>{p.schedule(reminder.schedule)} · set by {reminder.set_by === own ? "you" : reminder.set_by}</small></span><button className="dbtn warn" data-rem={reminder.id} aria-label="Delete reminder" aria-disabled={p.busy.has(`reminder:${reminder.id}`)} onClick={() => { if (!p.busy.has(`reminder:${reminder.id}`)) p.remove(reminder.id); }}>Delete</button></div>)}<div className="dfine">{ask}</div></div>}
    <div className="dsec flat">Your apps</div><div className="dtoggle"><span><b>Share my apps with Beep here</b><small>{g.apps_shared ? "Beep reads them to plan for the group and never shows them here. Sending anything asks you first." : "Only when you ask"}</small></span><button type="button" className="dswitch" role="switch" aria-label="Share my apps with Beep here" aria-checked={g.apps_shared} id="gapps" aria-disabled={p.busy.has("share")} onClick={() => { if (!p.busy.has("share")) p.share(); }} /></div>
    <div className="dsec flat">Privacy</div><div className="dinfo dmut"><div>Beep reads every member's memories but never quotes them here. Memories from email and Slack stay out of groups.</div><div>Beep's thinking and tool calls are hidden in groups. Members only see its replies.</div></div>
    {p.leave ? <div className="dcard"><div className="dask dline wrap"><span className="dfill">Leave {g.name}? You stop getting its messages, and need a new code to come back.</span><button className="dbtn quiet" id="gstay" onClick={p.stay}>Cancel</button><button className="dbtn warn" id="gleave" aria-disabled={p.busy.has("leave")} onClick={() => { if (!p.busy.has("leave")) p.leaveGroup(); }}>Leave</button></div></div> : <div><button className="dbtn warn" id="gaskleave" onClick={p.askLeave}>Leave group</button></div>}
  </>;
}
