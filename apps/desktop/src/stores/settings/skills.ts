// Settings > Skills: loads the skill libraries, the machines' problems with them and the
// libraries found in the agents' skills folders, and runs what the page asks.
import { createStore } from "zustand/vanilla";
import { invoke } from "../../ui/tauri.ts";
import { reason } from "../../ui/format.ts";
import type { FoundLibrary, LibraryProblem, Pick, SkillLibrary } from "./skills-state.ts";

type AddState = { kind: "closed" } | { kind: "open"; busy: boolean; error: string | null };
type FindState = { kind: "idle" } | { kind: "scanning" } | { kind: "found"; found: FoundLibrary[] } | { kind: "failed"; message: string };

export interface SkillsState {
  /// Null until the first read answers.
  libraries: SkillLibrary[] | null;
  /// Why the last read or change failed.
  error: string | null;
  problems: LibraryProblem[];
  /// The library whose skills are listed.
  open: string | null;
  add: AddState;
  find: FindState;
  /// A found library being added, by URL.
  adopting: string | null;
  /// Libraries with a change on its way.
  busy: string[];
}

const START: SkillsState = { libraries: null, error: null, problems: [], open: null, add: { kind: "closed" }, find: { kind: "idle" }, adopting: null, busy: [] };

const state = createStore<SkillsState>()(() => START);
const set = (part: Partial<SkillsState>) => state.setState(part);

const load = async () => {
  try {
    set({ libraries: await invoke<SkillLibrary[]>("skill_libraries_list"), error: null });
  } catch (error) {
    set({ error: reason(error) });
  }

  invoke<LibraryProblem[]>("skill_libraries_problems").then(problems => set({ problems }), error => console.warn("skill library problems:", error));
};

/// Runs a change to library `name`, then reads the list again.
const change = async (name: string, command: string, args: Record<string, unknown>) => {
  if (state.getState().busy.includes(name)) return;
  set({ busy: [...state.getState().busy, name], error: null });
  try {
    await invoke(command, { name, ...args });
  } catch (error) {
    set({ error: reason(error) });
  } finally {
    set({ busy: state.getState().busy.filter(n => n !== name) });
  }
  await load();
};

export const skills = {
  state,
  load,
  toggleOpen: (name: string) => set({ open: state.getState().open === name ? null : name }),
  openAdd: () => set({ add: { kind: "open", busy: false, error: null } }),
  closeAdd: () => { if (state.getState().add.kind === "open") set({ add: { kind: "closed" } }); },
  async add(text: string) {
    const add = state.getState().add, url = text.trim();
    if (add.kind !== "open" || add.busy) return;
    if (!url) return set({ add: { kind: "open", busy: false, error: "Type a Git URL." } });
    set({ add: { kind: "open", busy: true, error: null } });
    try {
      await invoke<SkillLibrary>("skill_library_add", { url, global: false });
      set({ add: { kind: "closed" } });
      await load();
    } catch (error) {
      set({ add: { kind: "open", busy: false, error: reason(error) } });
    }
  },
  setGlobal: (name: string, pick: Pick) => change(name, "skill_library_set_global", { pick }),
  setSkill: (name: string, skill: string, installed: boolean) => change(name, "skill_library_set_skill", { skill, installed }),
  uninstallProjects: (name: string) => change(name, "skill_library_uninstall_projects", {}),
  update: (name: string) => change(name, "skill_library_update", {}),
  async remove(name: string) {
    await invoke("skill_library_remove", { name });
    await load();
  },
  async find() {
    if (state.getState().find.kind === "scanning") return;
    set({ find: { kind: "scanning" } });
    try {
      set({ find: { kind: "found", found: await invoke<FoundLibrary[]>("skill_libraries_find") } });
    } catch (error) {
      set({ find: { kind: "failed", message: reason(error) } });
    }
  },
  /// Adds a found library installed globally: the nodes take over its skills where they are.
  async adopt(found: FoundLibrary) {
    if (state.getState().adopting) return;
    set({ adopting: found.url, error: null });
    try {
      await invoke<SkillLibrary>("skill_library_add", { url: found.url, global: true });
      dropFound(found.url);
      await load();
    } catch (error) {
      set({ error: reason(error) });
    } finally {
      set({ adopting: null });
    }
  },
  ignore: (url: string) => dropFound(url),
};

function dropFound(url: string) {
  const find = state.getState().find;
  if (find.kind === "found") set({ find: { kind: "found", found: find.found.filter(f => f.url !== url) } });
}
