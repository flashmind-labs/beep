// A project's or an assistant chat's Skills sheet: which library skills its checkouts get on
// every machine, or its folder and its threads' folders.
import { openScreen } from "../center/nav.ts";
import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { flushSync } from "react-dom";
import { openSheet } from "../ui/parts.tsx";
import { invoke } from "../ui/tauri.ts";
import { reason } from "../ui/format.ts";
import { Button, Checkbox, SheetHead } from "../ui/kit.tsx";
import { pickHas, placePicks, projectLines, toggleSkill, type Pick, type ProjectLibraries, type SkillLibrary, type SkillsPlace } from "../settings/skills-state.ts";

export const openProjectSkills = (project: string) => openSkills({ kind: "project", project }, `Skills in ${project}`);

/// The Skills sheet of the chat whose folder is named `chat`, called `title`.
export const openChatSkills = (chat: string, title: string) => openSkills({ kind: "chat", chat }, `Skills in ${title}`);

function openSkills(place: SkillsPlace, title: string) {
  let busy = false;
  const { close, render } = openSheet(title, "am-sheet", { canClose: () => !busy });
  const head = <SheetHead title={title} />;
  const save = async (picks: Record<string, Pick>, keepOutOfGit: boolean | null) => {
    busy = true;
    try {
      await (place.kind === "project"
        ? invoke("project_skills_set", { project: place.project, picks, keepOutOfGit: keepOutOfGit ?? false })
        : invoke("chat_skills_set", { chat: place.chat, picks }));
      busy = false;
      close();
    } catch (error) {
      busy = false;
      throw error;
    }
  };
  const manage = () => { if (!busy) { close(); openScreen({ screen: "settings", section: "skills" }); } };

  flushSync(() => render(<>{head}<div className="dmuted">Loading…</div></>));
  const load: Promise<Loaded> = place.kind === "project"
    ? invoke<ProjectLibraries>("project_skills_get", { project: place.project })
    : invoke<SkillLibrary[]>("skill_libraries_list").then(libraries => ({ libraries, keep_out_of_git: null }));
  load.then(
    loaded => flushSync(() => render(<SkillsView head={head} place={place} loaded={loaded} save={save} cancel={() => { if (!busy) close(); }} manage={manage} />)),
    error => flushSync(() => render(<>{head}<div className="am-msg bad" role="status">{reason(error)}</div><div className="am-line ip-acts"><Button look="quiet" onClick={close}>Close</Button></div></>)),
  );
}

/// The libraries, and whether a project keeps its installed skills out of Git; null for a chat.
interface Loaded { libraries: SkillLibrary[]; keep_out_of_git: boolean | null }

interface ViewProps {
  head: ReactNode; place: SkillsPlace; loaded: Loaded;
  save(picks: Record<string, Pick>, keepOutOfGit: boolean | null): Promise<void>; cancel(): void; manage(): void;
}

function SkillsView({ head, place, loaded, save, cancel, manage }: ViewProps) {
  const [picks, setPicks] = useState(() => placePicks(loaded.libraries, place));
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
    {keep !== null && <label className="am-line ps-git"><Checkbox checked={keep} disabled={busy} change={setKeep} />Keep out of Git<span className="ps-sub">{keep ? <>Listed in <code>.git/info/exclude</code></> : "Installed skills show as changes you can commit"}</span></label>}
    <div className="am-sub">Installed in <code>.claude/skills</code> and <code>.agents/skills</code> {place.kind === "project" ? "on every machine with this project" : "in this chat's folder and each of its threads'"}. Skills you wrote there stay.</div>
    <div className="am-msg bad" role="status" hidden={!error}>{error}</div>
    <div className="am-line ip-acts"><Button look="quiet" className="ps-manage" onClick={manage}>Manage libraries…</Button><Button look="quiet" onClick={cancel}>Cancel</Button><Button look="primary" ref={saveButton} disabled={busy} onClick={submit}>{busy ? "Saving…" : "Save"}</Button></div>
  </>;
}
