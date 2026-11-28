// Skill libraries (#674): the mesh records as the desktop reads them, and the sums Settings >
// Skills and a project's Skills sheet draw from. skills.ts loads and saves them.
import { plural } from "../../ui/format.ts";


/// Which of a library's skills a place gets, as serde writes `Pick`.
export type Pick = "None" | "All" | { Only: string[] };

export interface LibrarySkill { name: string; description: string }

/// One library (`skill_libraries_list`).
export interface SkillLibrary {
  name: string;
  /// The Git repository every machine clones.
  url: string;
  /// The commit every machine checks out.
  revision: string;
  skills: LibrarySkill[];
  global: Pick;
  /// By project (`org/repo`).
  projects: Record<string, Pick>;
  /// By the name of an assistant chat's folder.
  chats: Record<string, Pick>;
}

/// Where a Skills sheet picks for: a project's checkouts, or an assistant chat's folder and its
/// threads'.
export type SkillsPlace = { kind: "project"; project: string } | { kind: "chat"; chat: string };

/// What a machine could not fetch or install (`skill_libraries_problems`).
export interface LibraryProblem {
  machine: string;
  library: string;
  skill: string | null;
  /// `global` or a project name.
  place: string;
  message: string;
}

/// Skills on the machines from one repository that is no library yet (`skill_libraries_find`).
export interface FoundLibrary {
  url: string;
  name: string;
  skills: string[];
  places: { machine: string; folder: string }[];
}

/// The libraries for a project's sheet (`project_skills_get`).
export interface ProjectLibraries {
  libraries: SkillLibrary[];
  keep_out_of_git: boolean;
}

/// A checkbox for a whole library: every skill, some, or none.
export type Coverage = "all" | "some" | "none";

/// Where one skill of a library is installed.
export type SkillPlace = { kind: "global" } | { kind: "projects"; projects: string[] } | { kind: "none" };

export function pickHas(pick: Pick, skill: string) {
  if (pick === "None") return false;
  if (pick === "All") return true;
  return pick.Only.includes(skill);
}

/// How much of `skills` the pick covers.
export function coverage(pick: Pick, skills: readonly LibrarySkill[]): Coverage {
  const count = pickedCount(pick, skills);
  if (count === 0) return "none";
  return count === skills.length && skills.length > 0 ? "all" : "some";
}

function pickedCount(pick: Pick, skills: readonly LibrarySkill[]) {
  return skills.filter(skill => pickHas(pick, skill.name)).length;
}

/// `pick` with `skill` turned on or off. `All` turned off for one skill keeps every other one.
export function toggleSkill(pick: Pick, skill: string, on: boolean, skills: readonly LibrarySkill[]): Pick {
  const names = new Set(skills.filter(s => pickHas(pick, s.name)).map(s => s.name));
  if (on) names.add(skill);
  else names.delete(skill);
  if (!names.size) return "None";
  return { Only: skills.map(s => s.name).filter(name => names.has(name)) };
}

/// Where `skill` of `library` is installed: globally, in projects, or nowhere.
export function skillPlace(library: SkillLibrary, skill: string): SkillPlace {
  if (pickHas(library.global, skill)) return { kind: "global" };
  const projects = Object.entries(library.projects).filter(([, pick]) => pickHas(pick, skill)).map(([project]) => project).sort();
  return projects.length ? { kind: "projects", projects } : { kind: "none" };
}

export function placeText(place: SkillPlace) {
  switch (place.kind) {
    case "global": return "Global";
    case "projects": return place.projects.join(", ");
    case "none": return "Not installed";
  }
}

/// The projects that get any skill of `library`.
export function libraryProjects(library: SkillLibrary) {
  return Object.entries(library.projects).filter(([, pick]) => pick !== "None").map(([project]) => project).sort();
}

/// The chip saying where a library is installed, with its tooltip.
export function whereChip(library: SkillLibrary): { text: string; title: string | null } {
  if (library.global !== "None") {
    const some = coverage(library.global, library.skills) === "some";
    return { text: some ? `${pickedCount(library.global, library.skills)} global on every machine` : "Global on every machine", title: "In each agent's skills folder" };
  }

  const projects = libraryProjects(library);
  const chats = Object.values(library.chats).filter(pick => pick !== "None").length;
  if (!projects.length && !chats) return { text: "Not installed", title: null };
  if (!projects.length) return { text: `In ${plural(chats, "chat", "chats")}`, title: null };
  return { text: `In ${[plural(projects.length, "project", "projects"), ...chats ? [plural(chats, "chat", "chats")] : []].join(" and ")}`, title: projects.join(", ") };
}

/// The repository as the row shows it: without its scheme or `.git`.
export function urlText(url: string) {
  return url.replace(/^[a-z+]+:\/\//, "").replace(/\.git$/, "");
}

/// Where a found library sits: one folder and machine, or how many places.
export function foundWhere(found: FoundLibrary) {
  const [first] = found.places;
  if (!first) return "";
  if (found.places.length === 1) return `${first.folder} on ${first.machine}`;
  return `${found.places.length} skills folders`;
}

/// The problems of `library`, one line each.
export function problemLines(problems: readonly LibraryProblem[], library: string) {
  return problems.filter(p => p.library === library).map(p => {
    const about = [p.skill, p.place === "global" ? null : `in ${p.place}`].filter(part => part !== null).join(" ");
    return `${p.machine}: ${about ? `${about}: ` : ""}${p.message}`;
  });
}

/// One library's line in a project's Skills sheet.
export type ProjectLine =
  | { kind: "global"; library: SkillLibrary }
  | { kind: "pick"; library: SkillLibrary; pick: Pick; coverage: Coverage; sub: string };

/// The sheet's lines for `picks`, the project's pick of each library. A library installed
/// globally in full needs nothing in the project.
export function projectLines(libraries: readonly SkillLibrary[], picks: Readonly<Record<string, Pick>>): ProjectLine[] {
  return libraries.map(library => {
    if (coverage(library.global, library.skills) === "all") return { kind: "global", library };
    const pick = picks[library.name] ?? "None";
    const covered = coverage(pick, library.skills);
    const sub = covered === "some" ? `${pickedCount(pick, library.skills)} of ${plural(library.skills.length, "skill", "skills")}` : plural(library.skills.length, "skill", "skills");
    return { kind: "pick", library, pick, coverage: covered, sub };
  });
}

/// Each library's pick for `place`, as its sheet starts.
export function placePicks(libraries: readonly SkillLibrary[], place: SkillsPlace): Record<string, Pick> {
  const pick = (library: SkillLibrary) => (place.kind === "project" ? library.projects[place.project] : library.chats[place.chat]) ?? "None";
  return Object.fromEntries(libraries.map(library => [library.name, pick(library)]));
}
