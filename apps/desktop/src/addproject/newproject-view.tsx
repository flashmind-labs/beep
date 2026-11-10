import { useLayoutEffect, useRef, type MouseEvent } from "react";
import { canCreate, creatingText, goesText, landedLines, shownName, type Created, type Forge, type NewProjectForm } from "./newproject-state.ts";
import { ChipToggle } from "../ui/kit.tsx";

interface Machine { id: string; name: string; online: boolean }
interface Props {
  forges: Forge[] | null | undefined; loadError: string | null; form: NewProjectForm; machines: readonly Machine[];
  creating: boolean; error: string | null; created: Created | null; hostError: string | null;
  name(value: string): void; host(value: string | null): void; owner(event: MouseEvent<HTMLButtonElement>): void; privacy(value: boolean): void; machine(id: string): void;
  submit(): void; back(): void; close(): void; clone(url: string): void;
}
function Fields(p: Props) {
  const input = useRef<HTMLInputElement>(null), list = p.forges ?? [], owners = list.find(f => f.host === p.form.host)?.owners ?? [];
  const picked = p.form.machines.map(id => p.machines.find(machine => machine.id === id)?.name ?? id), note = goesText(p.form, picked);
  useLayoutEffect(() => { const field = input.current; if (field) { field.focus(); field.setSelectionRange(field.value.length, field.value.length); } }, []);
  return <><div className="am-label">Name</div><div className="am-line"><input ref={input} className="ip-field" type="text" placeholder="my-project" spellCheck={false} autoCapitalize="off" autoCorrect="off" aria-label="Name" value={p.form.name} onChange={e => p.name(e.currentTarget.value)} onKeyDown={e => { if (e.key === "Enter") { e.preventDefault(); p.submit(); } }} /></div>
    <div className="am-sub">{list.length ? "Create with a connected GitHub, Forgejo or Gitea account, or keep it local." : "Connect GitHub or add a Forgejo/Gitea token in Settings > Credentials to create a remote repository."}</div>
    <div className="am-label">Repository on</div><div className="am-line"><div className="dchips">{list.map(forge => <ChipToggle key={forge.host} label={forge.github ? "GitHub" : forge.host} selected={p.form.host === forge.host} onToggle={() => p.host(forge.host)} />)}<ChipToggle label="Local only" selected={p.form.host === null} onToggle={() => p.host(null)} /></div></div>
    {p.loadError && <div className="am-msg bad">{p.loadError}</div>}{p.hostError && <div className="am-msg bad">{p.hostError}</div>}
    {p.form.host !== null && owners.length > 0 && <><div className="am-label">Owner and visibility</div><div className="am-line"><button type="button" className="dbtn" onClick={p.owner}>{p.form.owner}</button><div className="dchips"><ChipToggle label="Private" selected={p.form.isPrivate} onToggle={() => p.privacy(true)} /><ChipToggle label="Public" selected={!p.form.isPrivate} onToggle={() => p.privacy(false)} /></div></div></>}
    <div className="am-label">Machines</div>{p.machines.length ? <div className="am-line"><div className="dchips">{p.machines.map(machine => <ChipToggle key={machine.id} label={machine.name} selected={p.form.machines.includes(machine.id)} onToggle={() => p.machine(machine.id)} />)}</div></div> : <div className="am-sub">None of your machines is online.</div>}
    {note && <div className="am-sub ip-to">{note}</div>}{p.error && <div className="am-msg bad">{p.error}</div>}<div className="am-line ip-foot"><button type="button" className="dbtn pri ip-go" disabled={!canCreate(p.form) || !!p.hostError} onClick={p.submit}>Create</button><button type="button" className="dbtn quiet" onClick={p.back}>Back</button></div></>;
}
function Done({ made, p }: { made: Created; p: Props }) {
  const first = made.imported[0];
  return <><div className="am-done"><span className="ddot" /><b>{shownName(p.form)} created</b></div>{first && landedLines(made.imported).map((line, index) => <div className="am-sub" key={index}>{line}</div>)}{made.failed.map((failure, index) => <div className="am-msg bad" key={index}>{failure.machine_name}: {failure.why}</div>)}{first ? <button type="button" className="dbtn pri" onClick={p.close}>Open project</button> : <div className="am-line"><button type="button" className="dbtn pri" onClick={() => p.clone(made.url)}>Clone…</button><button type="button" className="dbtn quiet" onClick={p.close}>Close</button></div>}</>;
}
export function NewProjectView(p: Props) {
  const names = p.form.machines.map(id => p.machines.find(machine => machine.id === id)?.name ?? id);
  return <><div className="am-head"><b>New project</b><kbd>esc</kbd></div>{p.forges === undefined ? <div className="am-wait"><span className="dspin" /></div> : p.forges === null ? <><div className="am-sub">The server needs an update before it creates projects.</div><div className="am-line ip-foot"><button type="button" className="dbtn quiet" onClick={p.back}>Back</button></div></> : p.creating ? <div className="am-wait"><span className="dspin" />{creatingText(p.form, names)}</div> : p.created ? <Done made={p.created} p={p} /> : <Fields {...p} />}</>;
}
