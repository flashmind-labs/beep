// The agent view with no session yet (⌘N, ⌘T in a project): a session's layout,
// header and side pane, with the shared new-agent composer centered in the
// empty conversation. Sending from it starts the session in place (session.ts
// openAgent with `adopt`); the pane and its terminals stay.
import { $, $$, bar, S, windowHooks } from "../bar/core.ts";
import { paneFrame, rebindPane, startPane } from "../agentpane/media.ts";
import { drawProjectComp, pointBar, projectComp } from "../project/composer.ts";
import type { BarPlace } from "../project/projectbar.ts";
import { pc } from "../project/sections.ts";
import { esc } from "../ui/format.ts";
import { AG } from "./agents.ts";
import { draftCwd } from "./draft-state.ts";
import { agentHints, drawEditors, stopAgentFollow } from "./session.ts";

/// Where the pane is bound: the machine and folder the input points to. Empty
/// until the frame is drawn, which starts the pane.
let bound = "";

/// Shows the empty view for `project`, on `machine` when it has a checkout
/// there, with the input focused. One already showing for the project is kept,
/// terminals included.
export function openDraft(project: string, machine: string | null) {
  const host = $("#agdraft");
  if (!(AG.draft === project && S.view === "agent" && host)) {
    pointBar(project, machine);
    stopAgentFollow();
    Object.assign(AG, { open: null, page: null, draft: project, inflight: null });
    S.view = "agent";
    drawFrame(project);
  }
  // The project page takes the input when it shows; it comes back here.
  const at = $$("#agdraft");
  if (projectComp.form.parentNode !== at) at.append(projectComp.form);
  drawProjectComp(project);
  projectComp.input.focus();
}

function drawFrame(project: string) {
  bar.innerHTML = `<div class="dhead"><span class="tt"><b><button class="agproj" id="agproj">${esc(project)}</button></b><small id="aghere"></small></span><span id="agedit" class="agedit"></span></div>
    <div class="agshell" id="agshell">${paneFrame(`<div class="agconv" id="agconv"><div class="dbody agdraft-body" id="agbody"><div id="agdraft"></div></div>${agentHints()}</div>`)}</div>`;
  $$("#agproj").addEventListener("click", () => windowHooks?.goBack({ view: "projects", arg: { project } }));
  $$("#agdraft").append(projectComp.form);
  bound = "";
  drawProjectComp(project);
}

/// The start input points at `place` (and the checkout it picked): the pane's
/// Files and Changes follow, and new terminals start there. Terminals already
/// open stay on their machines.
export function draftPlace(place: BarPlace | null) {
  const project = AG.draft;
  if (!project || project !== pc.project || !$("#agdraft")) return;
  const s = { id: "", machine_id: place?.machine_id ?? "", machine_name: place?.machine_name ?? "", cwd: draftCwd(place?.cwd ?? "", pc.checkout), agent: "", project };
  const key = `${s.machine_id}\n${s.cwd}`;
  if (key === bound) return;
  const first = bound === "";
  bound = key;
  if (first) startPane(s); else rebindPane(s);
  $$("#aghere").textContent = s.machine_name;
  void drawEditors(s.machine_id, s.cwd, () => AG.draft === project && bound === key);
}
