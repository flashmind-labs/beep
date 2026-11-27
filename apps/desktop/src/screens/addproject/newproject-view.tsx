import { useLayoutEffect, useRef, type MouseEvent } from "react";
import { canCreate, creatingText, folderOn, folderTail, goesText, landedLines, shownName, type Created, type Forge, type NewProjectForm } from "../../stores/addproject/newproject-state.ts";
import { Button, ChipToggle, NO_AUTO, SheetHead, Spinner } from "../../ui/kit.tsx";
import { FolderField, pickFolder } from "./folderbrowser.tsx";

interface Machine { id: string; name: string; online: boolean }
interface Props {
  forges: Forge[] | undefined; loadError: string | null; form: NewProjectForm; machines: readonly Machine[]; defaults: ReadonlyMap<string, string>;
  creating: boolean; error: string | null; created: Created | null; hostError: string | null;
  name(value: string): void; host(value: string | null): void; owner(event: MouseEvent<HTMLButtonElement>): void; privacy(value: boolean): void; machine(id: string): void; folder(id: string, folder: string): void;
  submit(): void; back(): void; close(): void; clone(url: string): void;
}
function Fields(p: Props) {
  const input = useRef<HTMLInputElement>(null), list = p.forges ?? [], owners = list.find(f => f.host === p.form.host)?.owners ?? [];
  const note = goesText(p.form), tail = folderTail(p.form);
  useLayoutEffect(() => { const field = input.current; if (field) { field.focus(); field.setSelectionRange(field.value.length, field.value.length); } }, []);
  return <><div className="am-label">Name</div><div className="am-line"><input ref={input} className="ip-field" type="text" placeholder="my-project" {...NO_AUTO} aria-label="Name" value={p.form.name} onChange={e => p.name(e.currentTarget.value)} onKeyDown={e => { if (e.key === "Enter") { e.preventDefault(); p.submit(); } }} /></div>
    <div className="am-sub">{list.length ? "Create with a connected GitHub, Forgejo or Gitea account, or keep it local." : "Connect GitHub or add a Forgejo/Gitea token in Settings > Credentials to create a remote repository."}</div>
    <div className="am-label">Repository on</div><div className="am-line"><div className="dchips">{list.map(forge => <ChipToggle key={forge.host} label={forge.github ? "GitHub" : forge.host} selected={p.form.host === forge.host} onToggle={() => p.host(forge.host)} />)}<ChipToggle label="Local only" selected={p.form.host === null} onToggle={() => p.host(null)} /></div></div>
    {p.loadError && <div className="am-msg bad">{p.loadError}</div>}{p.hostError && <div className="am-msg bad">{p.hostError}</div>}
    {p.form.host !== null && owners.length > 0 && <div className="am-line np-repository">
      <div role="group" aria-labelledby="np-owner-label">
        <div id="np-owner-label" className="am-label">Account or organization</div>
        <Button onClick={p.owner} title={p.form.owner}>{p.form.owner}</Button>
      </div>
      <div role="group" aria-labelledby="np-visibility-label">
        <div id="np-visibility-label" className="am-label">Visibility</div>
        <div className="dchips">
          <ChipToggle label="Private" selected={p.form.isPrivate} onToggle={() => p.privacy(true)} />
          <ChipToggle label="Public" selected={!p.form.isPrivate} onToggle={() => p.privacy(false)} />
        </div>
      </div>
    </div>}
    <div className="am-label">Machines</div>{p.machines.length ? <div className="am-line"><div className="dchips">{p.machines.map(machine => <ChipToggle key={machine.id} label={machine.name} selected={p.form.machines.includes(machine.id)} onToggle={() => p.machine(machine.id)} />)}</div></div> : <div className="am-sub">None of your machines is online.</div>}
    {p.form.machines.length > 0 && <><div className="am-label">Folders</div>{p.form.machines.map(id => { const name = p.machines.find(machine => machine.id === id)?.name ?? id; return <div className="am-line np-folder" key={id}><span className="np-m" title={name}>{name}</span><FolderField machine={id} value={folderOn(p.form, id, p.defaults)} label={`Folder on ${name}`} placeholder="The machine's projects folder" disabled={false} change={folder => p.folder(id, folder)} enter={p.submit} /><Button look="quiet" onClick={() => void pickFolder({ machine: id, start: folderOn(p.form, id, p.defaults) || null, heading: `Choose a folder on ${name}` }).then(listing => { if (listing) p.folder(id, listing.path); })}>Choose…</Button><span className="np-to" title={tail}>{tail}</span></div>; })}</>}
    {note && <div className="am-sub ip-to">{note}</div>}{p.error && <div className="am-msg bad">{p.error}</div>}<div className="am-line ip-foot"><Button look="primary" className="ip-go" disabled={!canCreate(p.form) || !!p.hostError} onClick={p.submit}>Create</Button><Button look="quiet" onClick={p.back}>Back</Button></div></>;
}
function Done({ made, p }: { made: Created; p: Props }) {
  const first = made.imported[0];
  return <><div className="am-done"><span className="ddot" /><b>{shownName(p.form)} created</b></div>{first && landedLines(made.imported).map((line, index) => <div className="am-sub" key={index}>{line}</div>)}{made.failed.map((failure, index) => <div className="am-msg bad" key={index}>{failure.machine_name}: {failure.why}</div>)}{first ? <Button look="primary" onClick={p.close}>Open project</Button> : <div className="am-line"><Button look="primary" onClick={() => p.clone(made.url)}>Clone…</Button><Button look="quiet" onClick={p.close}>Close</Button></div>}</>;
}
export function NewProjectView(p: Props) {
  const names = p.form.machines.map(id => p.machines.find(machine => machine.id === id)?.name ?? id);
  return <><SheetHead title="New project" />{p.forges === undefined ? <div className="am-wait"><Spinner /></div> : p.creating ? <div className="am-wait"><Spinner />{creatingText(p.form, names)}</div> : p.created ? <Done made={p.created} p={p} /> : <Fields {...p} />}</>;
}
