import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { useStore } from "zustand";
import { Button, NO_AUTO, Spinner } from "../../ui/kit.tsx";
import { Icon } from "../../ui/icons.tsx";
import { confirmSheet, openMenu, type MenuItem } from "../../ui/menu.tsx";
import { skills, type SkillsState } from "../../stores/settings/skills.ts";
import { foundWhere, libraryProjects, placeText, plural, problemLines, skillPlace, urlText, whereChip, type FoundLibrary, type SkillLibrary } from "../../stores/settings/skills-state.ts";

export function SkillsView() {
  const state = useStore(skills.state);
  useEffect(() => { void skills.load(); }, []);

  if (state.libraries === null) {
    if (state.error) return <div className="derr" role="alert">{state.error} <Button look="quiet" small onClick={() => void skills.load()}>Retry</Button></div>;
    return <div className="dskel-rows"><div className="dskel" style={{ width: "78%" }} /><div className="dskel" style={{ width: "56%" }} /></div>;
  }

  const actions = <div className="dline tight wrap dmt6"><Button small onClick={skills.openAdd}>+ Add a library</Button><Button look="quiet" small disabled={state.find.kind === "scanning"} onClick={() => void skills.find()}>Find existing libraries</Button></div>;
  return <div className="dcreds" id="skillLibraries">
    {state.error && <div className="derr" role="alert">{state.error}</div>}
    {state.libraries.length === 0 && state.add.kind === "closed" && <span className="dsub">A library is a pack of skills from a Git repository. Install it in the projects that need it, or globally.</span>}
    {state.libraries.map(library => <LibraryRow key={library.name} library={library} state={state} />)}
    {state.add.kind === "open" ? <AddForm busy={state.add.busy} error={state.add.error} /> : actions}
    <Found state={state} />
  </div>;
}

function LibraryRow({ library, state }: { library: SkillLibrary; state: SkillsState }) {
  const open = state.open === library.name, busy = state.busy.includes(library.name);
  const where = whereChip(library), problems = problemLines(state.problems, library.name);
  const acts = <>{busy && <Spinner />}<Button look="quiet" small aria-expanded={open} onClick={() => skills.toggleOpen(library.name)}>{open ? "Hide skills" : "Skills"}</Button><Button look="quiet" small aria-label={`More for ${library.name}`} aria-haspopup="menu" disabled={busy} onClick={event => openMenu(event.nativeEvent, library.name, libraryMenu(library))}>&#8943;</Button></>;
  return <Row name={library.name} account={urlText(library.url)} acts={acts}
    chips={<><Chip>{plural(library.skills.length, "skill")}</Chip><Chip title={where.title}>{where.text}</Chip><Chip title="Commit">{library.revision.slice(0, 7)}</Chip></>}>
    {open && <div className="dskill-list">{library.skills.map(skill => {
      const place = skillPlace(library, skill.name);
      return <div className="dskill-line" key={skill.name}><code>{skill.name}</code><span className="desc">{skill.description}</span><span className="at">{placeText(place)}</span>
        {place.kind === "none"
          ? <Button look="quiet" small disabled={busy} onClick={() => void skills.setSkill(library.name, skill.name, true)}>Install globally</Button>
          : <Button look="quiet" small disabled={busy} onClick={() => void skills.setSkill(library.name, skill.name, false)}>Uninstall</Button>}</div>;
    })}</div>}
    {problems.map(line => <span className="derr dskill-problem" key={line}>{line}</span>)}
  </Row>;
}

function libraryMenu(library: SkillLibrary): MenuItem[] {
  const projects = libraryProjects(library), global = library.global !== "None";
  return [
    { label: global ? "Uninstall globally" : "Install globally", on: () => void skills.setGlobal(library.name, global ? "None" : "All") },
    ...(projects.length ? [{ label: `Uninstall from ${plural(projects.length, "project")}`, on: () => void skills.uninstallProjects(library.name) }] : []),
    { label: "Update", on: () => void skills.update(library.name) },
    { kind: "separator" },
    { label: "Remove library", warn: true, on: () => confirmSheet({
      title: `Remove ${library.name}?`,
      text: "Every machine uninstalls its skills, globally and in projects. Skills you wrote yourself stay.",
      action: "Remove",
      run: () => skills.remove(library.name),
    }) },
  ];
}

function AddForm({ busy, error }: { busy: boolean; error: string | null }) {
  const [text, setText] = useState("");
  const input = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => { input.current?.focus(); }, []);
  const add = () => void skills.add(text);
  return <div className="dcred-form"><b>Add a library</b>
    <div className={`dfield${error ? " err" : ""}`}><div className="dline">
      <input ref={input} className="dskill-source" placeholder="Git URL" {...NO_AUTO} aria-label="Git URL" value={text} disabled={busy}
        onChange={event => setText(event.currentTarget.value)} onKeyDown={event => { if (event.key === "Enter") { event.preventDefault(); add(); } }} />
      <Button look="primary" disabled={busy} onClick={add}>{busy ? "Reading…" : "Add"}</Button><Button look="quiet" onClick={skills.closeAdd}>Cancel</Button>
    </div></div>
    <span className="dsub">Each folder with a <code>SKILL.md</code> is one skill. The library name is the repository's name.</span>
    {error && <div className="derr" role="alert">{error}</div>}
  </div>;
}

function Found({ state }: { state: SkillsState }) {
  const find = state.find;
  switch (find.kind) {
    case "idle": return null;
    case "scanning": return <><div className="dcred-head">Found on your machines</div><div className="dmuted"><Spinner />Looking in each agent's skills folder on your machines…</div></>;
    case "failed": return <><div className="dcred-head">Found on your machines</div><div className="derr">{find.message}</div></>;
    case "found": return <><div className="dcred-head">Found on your machines</div>
      {find.found.length === 0
        ? <span className="dsub">No other skills from a Git repository.</span>
        : <span className="dsub">Global skills that come from a Git repository, grouped by repository. Adding one keeps its skills installed globally and updates them from the repository.</span>}
      {find.found.map(found => <FoundRow key={found.url} found={found} adopting={state.adopting} />)}
    </>;
  }
}

function FoundRow({ found, adopting }: { found: FoundLibrary; adopting: string | null }) {
  const busy = adopting === found.url;
  return <Row name={found.name} account={urlText(found.url)}
    acts={<><Button small disabled={adopting !== null} onClick={() => void skills.adopt(found)}>{busy ? "Adding…" : "Add as library"}</Button><Button look="quiet" small disabled={busy} onClick={() => skills.ignore(found.url)}>Ignore</Button></>}
    chips={<><Chip>{plural(found.skills.length, "skill")}</Chip><Chip title={found.places.map(p => `${p.folder} on ${p.machine}`).join("\n")}>{foundWhere(found)}</Chip></>} />;
}

function Row({ name, account, acts, chips, children }: { name: string; account: string; acts: ReactNode; chips: ReactNode; children?: ReactNode }) {
  return <div className="dgit" data-library={name}><span className="ic"><span><Icon name="book" /></span></span><div className="l1"><b>{name}</b><span className="acct">{account}</span><span className="acts">{acts}</span></div><div className="l2">{chips}</div>{children}</div>;
}

function Chip({ title = null, children }: { title?: string | null; children: ReactNode }) {
  return <span className="dgchip" title={title ?? undefined}>{children}</span>;
}
