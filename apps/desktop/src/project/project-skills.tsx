// A project's Skills sheet: which library skills its checkouts get on every machine.
import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { flushSync } from "react-dom";
import { openSheet } from "../ui/parts.tsx";
import { invoke } from "../ui/tauri.ts";
import { reason } from "../ui/format.ts";
import { Button, Checkbox, SheetHead } from "../ui/kit.tsx";
import { openSettings } from "../settings/sheet.ts";
import { pickHas, projectLines, projectPicks, toggleSkill, type Pick, type ProjectLibraries } from "../settings/skills-state.ts";

export function openProjectSkills(project: string) {
  let busy = false;
  const { close, render } = openSheet(`Skills in ${project}`, "am-sheet", { canClose: () => !busy });
  const head = <SheetHead title={`Skills in ${project}`} />;
  const save = async (picks: Record<string, Pick>, keepOutOfGit: boolean) => {
    busy = true;
    try {
      await invoke("project_skills_set", { project, picks, keepOutOfGit });
      busy = false;
      close();
    } catch (error) {
      busy = false;
      throw error;
    }
  };
  const manage = () => { if (!busy) { close(); openSettings("skills"); } };

  flushSync(() => render(<>{head}<div className="dmuted">Loading…</div></>));
  invoke<ProjectLibraries>("project_skills_get", { project }).then(
    loaded => flushSync(() => render(<ProjectSkillsView head={head} project={project} loaded={loaded} save={save} cancel={() => { if (!busy) close(); }} manage={manage} />)),
    error => flushSync(() => render(<>{head}<div className="am-msg bad" role="status">{reason(error)}</div><div className="am-line ip-acts"><Button look="quiet" onClick={close}>Close</Button></div></>)),
  );
}

interface ViewProps {
  head: ReactNode; project: string; loaded: ProjectLibraries;
  save(picks: Record<string, Pick>, keepOutOfGit: boolean): Promise<void>; cancel(): void; manage(): void;
}

function ProjectSkillsView({ head, project, loaded, save, cancel, manage }: ViewProps) {
  const [picks, setPicks] = useState(() => projectPicks(loaded.libraries, project));
  const [keep, setKeep] = useState(loaded.keep_out_of_git);
  const [open, setOpen] = useState<string | null>(null);
  const [busy, setBusy] = useState(false), [error, setError] = useState("");
  const saveButton = useRef<HTMLButtonElement>(null);
  useLayoutEffect(() => { saveButton.current?.focus(); }, []);
  const pick = (name: string, value: Pick) => setPicks(current => ({ ...current, [name]: value }));
  const submit = () => {
    if (busy) return;
    setBusy(true);
    setError("");
    save(picks, keep).catch(e => { setError(reason(e)); setBusy(false); });
  };

  return <>{head}
    {loaded.libraries.length === 0 && <div className="am-sub">No skill libraries yet. Add one in Settings.</div>}
    {projectLines(loaded.libraries, picks).map(line => {
      const library = line.library;
      if (line.kind === "global") return <div className="am-line ps-lib" key={library.name}><Checkbox checked disabled label={library.name} change={() => {}} /><b>{library.name}</b><span className="ps-sub">Installed globally</span></div>;
      const shown = open === library.name;
      return <div key={library.name}>
        <div className="am-line ps-lib"><Checkbox checked={line.coverage === "all"} mixed={line.coverage === "some"} disabled={busy} label={library.name} change={on => pick(library.name, on ? "All" : "None")} /><b>{library.name}</b><span className="ps-sub">{line.sub}</span>
          {library.skills.length > 1 && <Button look="quiet" small className="ps-choose" aria-expanded={shown} onClick={() => setOpen(shown ? null : library.name)}>{shown ? "Hide" : "Choose skills"}</Button>}</div>
        {shown && library.skills.map(skill => <label className="am-line ps-skill" key={skill.name}><Checkbox checked={pickHas(line.pick, skill.name)} disabled={busy} label={skill.name} change={on => pick(library.name, toggleSkill(line.pick, skill.name, on, library.skills))} /><code>{skill.name}</code><span className="ps-sub">{skill.description}</span></label>)}
      </div>;
    })}
    <label className="am-line ps-git"><Checkbox checked={keep} disabled={busy} change={setKeep} />Keep out of Git<span className="ps-sub">{keep ? <>Listed in <code>.git/info/exclude</code></> : "Installed skills show as changes you can commit"}</span></label>
    <div className="am-sub">Installed in <code>.claude/skills</code> and <code>.agents/skills</code> on every machine with this project. Skills you wrote there stay.</div>
    <div className="am-msg bad" role="status" hidden={!error}>{error}</div>
    <div className="am-line ip-acts"><Button look="quiet" className="ps-manage" onClick={manage}>Manage libraries…</Button><Button look="quiet" onClick={cancel}>Cancel</Button><Button look="primary" ref={saveButton} disabled={busy} onClick={submit}>{busy ? "Saving…" : "Save"}</Button></div>
  </>;
}
