// A project page's file sync, workflow runs and live mode, with the agent buttons they offer.
import { projectRows } from "./projectrows.ts";
import { projectChanged } from "./barstate.ts";
import { railProject, shell } from "../center/window-shell.tsx";
import { pageShown } from "../ui/active.ts";
import { subscribeEach, topic } from "../ui/mesh-events.ts";
import { pushedEach } from "../ui/snapshot-state.ts";
import { layoutChanged } from "../ui/skin.ts";
import { onPlaces } from "../agents/place.ts";
import { onMoves } from "../agents/move-store.ts";
import { onClosing } from "../agents/closing.ts";
import { invoke, listen, openUrl } from "../ui/tauri.ts";
import { localProjectId } from "./localid.ts";
import { followedProject, makeProjectLive, type LiveRow } from "./projectlive.ts";
import { makeFileSync, type SyncDetail } from "./filesync.ts";
import type { ListsRow } from "./threads.ts";
import { makeRuns, type RunDetail, type RunsRow } from "./runs.tsx";
import { state, keptSection, projectLoaded, syncChanged } from "../center/state.ts";
import { currentLocation } from "../center/nav.ts";
import { fixSession, sessionState, fixRun, startFix, fixConflict, projectButtons, startConflictFix, startSyncRepair } from "./composer.tsx";
import { thr } from "./stores.ts";

export const fsync = makeFileSync({
  invoke, redraw: () => { projectChanged.notify(); syncChanged.notify(); }, fix: fixConflict, kept: keptSection("filesync"),
  fixButton: (project, c) => projectButtons(`conflict ${project}\n${c.path}`, { project, machine: c.holder, choose: false, label: "Fix with an agent", hint: "Pick a model: an agent merges the two copies on the machine that has them", session: null, start: pick => startConflictFix(project, c, pick) }),
  repairButton: (project, machine, error) => repairButton(project, machine, error),
});

// Each project's sync detail comes with the `sync` updates, as the machines hold it.
subscribeEach(topic("sync"), event => {
  for (const { args, pushed } of pushedEach<SyncDetail | null>(event.data, "project_sync_detail")) {
    if (typeof args.project === "string" && "value" in pushed && pushed.value) fsync.take(args.project, pushed.value);
  }
});

/// Fix with an agent on a blocked Git sync of `project` on `machine`.
export function repairButton(project: string, machine: string, error: string) {
  return projectButtons(`repair ${project}\n${machine}`, { project, machine, choose: false, label: "Fix with an agent", hint: "Pick a model: an agent starts here with both checkouts and recent sessions in its prompt", session: null, start: pick => startSyncRepair(project, machine, error, pick) });
}

/// Whether the project shown is live or polling; what its host changed is
/// read again as the mesh says.
export const liveMode = makeProjectLive({
  invoke,
  redraw: () => projectChanged.notify(),
  reload: (id, update) => {
    const lists = update.lists && update.view<ListsRow | null>("center_project_lists", { projectId: id });
    if (lists) thr.take(id, lists);

    if (update.runs) runs.take(id, update.view<RunsRow>("center_project_runs", { projectId: id }),
      run => update.view<RunDetail>("center_project_run", { projectId: id, runId: run.id, host: run.host ?? null }));
  },
});
listen<LiveRow>("project-live", e => liveMode.heard(e.payload));

/// The project's workflow runs: the Actions section and each PR's checks.
export const runs = makeRuns({
  invoke,
  redraw: () => projectChanged.notify(),
  openUrl,
  fix: (detail: RunDetail) => { if (state.picked) fixRun(state.picked, detail); },
  startFix: (name, detail, pick, machine) => startFix(name, detail, pick, machine),
  fixSession: (name, detail) => sessionState(fixSession(name, detail), "Fixing"),
  button: (key, o) => projectButtons(key, o),
  mode: liveMode.mode,
  kept: keptSection("runs"),
  hostsChanged: () => projectLoaded.notify(),
});

layoutChanged.add(() => projectChanged.notify());
onPlaces(() => projectChanged.notify());
onMoves(() => projectChanged.notify());
onClosing(() => projectChanged.notify());

/// Follows the imported project this window shows, on its page or in its rail, while visible.
export function syncProjectFollow() {
  const project = followedProject(
    pageShown(),
    currentLocation()?.view ?? null,
    state.picked,
    railProject(shell.getState().side.rail),
    projectRows(),
  );
  void liveMode.follow(project ? localProjectId(project) : null);
}
