// A project's team as last read: its record and the board's team cards, who holds an issue, and
// taking one.
import { keptSection } from "../center/state.ts";
import { reason } from "../ui/format.ts";
import { subscribeEach, topic } from "../ui/mesh-events.ts";
import { invoke, listen } from "../ui/tauri.ts";
import { localProjectId } from "./localid.ts";
import { redraw } from "../center/redraw.ts";
import { thr } from "./stores.ts";
import { claim, heldText, onlineCount, teamView, type TeamCards, type TeamRecord, type TeamView } from "./team-state.ts";

const NO_CARDS: TeamCards = { needs_you: [], teammates: [], own_queued: [], online: [] };

/// What a project's team read last, kept between windows so it shows at once.
interface KeptTeam { record: TeamRecord | null; cards: TeamCards }
const kept = keptSection<KeptTeam>("team");

/// Each project's team by name: read, taken from the cache or pushed. A project
/// missing has not answered yet, or is on no forge.
const views = new Map<string, TeamView>();
const cards = new Map<string, TeamCards>();
const reading = new Map<string, Promise<void>>();
/// The project each id belongs to, for a push, which names the project by id.
const names = new Map<string, string>();
/// What each project's team last held, to draw only on a change.
const answers = new Map<string, string>();
/// The projects whose first read failed: a project on no forge fails every time. The next
/// projects push, which says a record may have landed, asks again.
const failed = new Set<string>();

/// Project `name`'s team as last read, or null before it answered; asks for it the first time.
export function teamOf(name: string): TeamView | null {
  const known = views.get(name);
  if (known) return known;

  const saved = kept.get(localProjectId(name));
  if (saved) {
    const savedCards = { ...NO_CARDS, ...saved.cards };
    answers.set(name, JSON.stringify({ record: saved.record, cards: savedCards }));
    views.set(name, teamView(saved.record));
    cards.set(name, savedCards);
  }
  if (!reading.has(name) && !failed.has(name)) void readTeam(name);

  return views.get(name) ?? null;
}

/// The cards project `name`'s team adds to its board, as last read.
export const teamCardsOf = (name: string) => cards.get(name) ?? NO_CARDS;

/// Takes the team of project `name` as its record and cards, and draws the page when they changed.
function takeTeam(name: string, record: TeamRecord | null, held: TeamCards) {
  const projectId = localProjectId(name);
  const answer = JSON.stringify({ record, cards: held });
  failed.delete(name);
  if (answers.get(name) === answer && views.has(name)) return;

  answers.set(name, answer);
  views.set(name, teamView(record));
  cards.set(name, held);
  kept.put(projectId, { record, cards: held });
  redraw("projects");
}

/// Reads project `name`'s team and, when it has one, its cards, once; the pushes follow after.
export function readTeam(name: string): Promise<void> {
  const running = reading.get(name);
  if (running) return running;

  const read = (async () => {
    const projectId = localProjectId(name);
    names.set(projectId, name);

    try {
      const record = await invoke<TeamRecord | null>("team_record", { projectId });
      const view = teamView(record);
      const read = view.kind === "team" ? await invoke<TeamCards>("team_cards", { projectId }).then(got => ({ ...NO_CARDS, ...got })).catch(error => {
        console.warn("team cards:", reason(error));
        return teamCardsOf(name);
      }) : NO_CARDS;
      takeTeam(name, record, read);
    } catch (error) {
      // A project on no forge has no team; the page leaves the Team row out.
      failed.add(name);
      console.warn("team:", reason(error));
    } finally {
      reading.delete(name);
    }
  })();
  reading.set(name, read);

  return read;
}

/// A change of a project's team, from the desktop: its record and board cards as they stand now.
/// A record of `null` is no team; cards of `null` leave the cards held.
interface TeamChanged { project_id: string; record: TeamRecord | null; cards: TeamCards | null }

subscribeEach(topic("projects"), () => {
  if (!failed.size) return;
  failed.clear();
  redraw("projects");
});

listen<TeamChanged>("team-changed", ({ payload }) => {
  const name = names.get(payload.project_id);
  if (name === undefined) return;

  takeTeam(name, payload.record, payload.record === null ? NO_CARDS : payload.cards ? { ...NO_CARDS, ...payload.cards } : teamCardsOf(name));
});

/// The Team pill and row's note: who is online, empty without a team (both
/// still show so the user can invite or join), or null before the team
/// answered (which leaves both out).
export function teamNote(name: string) {
  const view = teamOf(name);
  if (!view) return null;

  return view.kind === "team" ? `${onlineCount(view, teamCardsOf(name).online)} online` : "";
}

/// The logins of project `project`'s team, for the Assign menu; none without a team.
export function teamLogins(project: string) {
  const view = teamOf(project);
  return view?.kind === "team" ? view.record.members.map(member => member.login) : [];
}

/// Assigns `logins` to issue `number` of `project`, none for the user, and reads the issues again.
export async function assign(project: string, number: number, logins: readonly string[]) {
  const projectId = localProjectId(project);
  await invoke("issue_assign", { projectId, number, logins });
  void thr.load(projectId, true);
}

/// Why issue `number` of `project` is left out of a batch start: someone else has it; null when free to take.
export function heldBy(project: string, number: number): string | null {
  const view = teamOf(project), issue = thr.issues(localProjectId(project))?.find(each => each.number === number);
  if (view?.kind !== "team" || !issue) return null;

  const taken = claim(issue, view.me);
  return taken.kind === "held" ? heldText(taken.by, number) : null;
}

/// Before a batch start of issue `number` of `project`, with the team read
/// first: why it is left out when someone else has it, else null, the user
/// taking it when it is free.
export async function takeFree(project: string, number: number): Promise<string | null> {
  if (!teamOf(project)) await readTeam(project);
  const held = heldBy(project, number);
  if (held !== null) return held;

  const view = teamOf(project), issue = thr.issues(localProjectId(project))?.find(each => each.number === number);
  if (view?.kind === "team" && issue && claim(issue, view.me).kind === "free") await assign(project, number, []);
  return null;
}
