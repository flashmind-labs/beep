// The agent view with no session yet (⌘N, ⌘T in a project): a session's layout,
// header and side pane, with the shared new-agent composer centered in the
// empty conversation. Sending from it starts the session in place (session.ts
// openSession with `adopt`); the pane and its terminals stay.
import { createStore } from "zustand/vanilla";
import { S, setView } from "../../stores/bar/core.ts";
import { goBack } from "./leave.ts";
import { projectWindowProps } from "../project/projectops.ts";
import { rebindPane, startPane } from "./media.ts";
import { drawProjectComp, pointBar, projectComp } from "../project/composer.tsx";
import type { BarPlace } from "../../stores/project/projectbar.ts";
import { barPlaced, pc } from "../../stores/project/barstate.ts";
import { AG } from "../../stores/agents/agents-state.ts";
import type { ChatBody } from "../../ui/turn.ts";
import { DraftHead } from "./session-frame.tsx";
import { closeSession } from "./open-state.ts";
import { draftMessage, showSessionFrame } from "../../stores/agents/session-frame-state.ts";
import { draftCwd } from "../../stores/agents/draft-state.ts";
import { agentHints, drawEditors } from "./session-head.tsx";
import { stopAgentFollow } from "./session-follow.ts";
import { transcripts } from "../../stores/agents/transcript-cache-state.ts";
import { leaveLog } from "./session.tsx";
import { clearPage } from "../../stores/agents/open-page.ts";

/// Where the pane is bound: the machine and folder the input points to. Empty
/// until the frame is drawn, which starts the pane.
let bound = "";
/// The empty conversation the composer sits in, while the draft shows.
let host: HTMLDivElement | null = null;
/// The machine the head names.
const machine = createStore<string>(() => "");
const shown = () => !!host?.isConnected;

/// Shows the empty view for `project`, on `machine` when it has a checkout
/// there, with the input focused. One already showing for the project is kept,
/// terminals included.
export function openDraft(project: string, on: string | null) {
  if (!(AG.draft === project && S.view === "agent" && shown())) {
    pointBar(project, on);
    stopAgentFollow();
    leaveLog();
    clearPage();
    Object.assign(AG, { open: null, draft: project, inflight: null });
    transcripts.pin(null);
    closeSession();
    setView("agent");
    drawFrame(project);
  }

  // The project page takes the input when it shows; it comes back here.
  if (host && projectComp().form.parentNode !== host) host.append(projectComp().form);
  drawProjectComp(project);
  draftMessage.current = projectComp().input;
  projectComp().input.focus();
}

/// The frame's log body is a plain element, as a session's is, so the draft
/// keeps the same layout and its pane stays when the session starts.
function drawFrame(project: string) {
  const body: ChatBody = document.createElement("div");
  body.className = "dbody agdraft-body"; body.id = "agbody";
  const draft = host = document.createElement("div"); draft.id = "agdraft"; body.append(draft);
  const openProject = () => goBack({ view: "projects", arg: { project } }, true);
  machine.setState("", true);
  showSessionFrame({ head: <DraftHead windowProps={projectWindowProps(project)} project={project} openProject={openProject} machine={machine} />, field: null, body, hints: agentHints() }, false, () => { draftMessage.current = null; });
  draft.append(projectComp().form);
  bound = "";
  drawProjectComp(project);
}

/// The start input points at `place` (and the checkout it picked): the pane's
/// Files and Changes follow, and new terminals start there. Terminals already
/// open stay on their machines.
function draftPlace(place: BarPlace | null) {
  const project = AG.draft;
  if (!project || project !== pc.project || !shown()) return;
  const s = { id: "", machine_id: place?.machine_id ?? "", machine_name: place?.machine_name ?? "", cwd: draftCwd(place?.cwd ?? "", pc.checkout), agent: "", project };
  const key = `${s.machine_id}\n${s.cwd}`;
  if (key === bound) return;
  const first = bound === "";
  bound = key;
  if (first) startPane(s); else rebindPane(s);
  machine.setState(s.machine_name, true);
  void drawEditors(s.machine_id, s.cwd, () => AG.draft === project && bound === key);
}
barPlaced.add(draftPlace);
