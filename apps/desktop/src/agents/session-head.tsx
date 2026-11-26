// The open session's head and key hints: its title, folder, usage and the
// editors that open its folder.
import { agentMessage, removeSessionPortal, sessionEditHost, setSessionPortal, updateSessionHints } from "./session-frame-state.ts";
import { updateSessionHead } from "./session-frame.tsx";
import { PANE } from "../bar/core.ts";
import { sessionProject } from "../project/sessionproject.ts";
import { hintModel, type Hint } from "../bar/hints.ts";
import { type AgentRow, AG, openModes, sessionUsage } from "./agents-state.ts";
import { agentKind, folderName } from "./agent-kind.ts";
import { editorsFor, openInEditor } from "../project/editors.ts";
import { ProjectEditors } from "../project/header-view.tsx";
import { ICON } from "../ui/icons.tsx";
import { invoke } from "../ui/tauri.ts";
import { onPlaces, placeOf } from "./place.ts";
import { placeName, placeRepo } from "./place-state.ts";
import { agentKey } from "./conversation-state.ts";
import { isNativeSession } from "./native-session-state.ts";
import { isStarting } from "./starting-state.ts";
import { followsOnly, hasModes, stoppingNow, turnRuns } from "./open-state.ts";

/// The session's keys on one small line.
export function agentHints() {
  const s = AG.page?.session, q = s?.question;
  const slim = (items: readonly Hint[]) => {
    const project = AG.draft ?? (s && sessionProject(s)?.name) ?? null;
    return { value: { ...hintModel(items), window: !PANE && !!s && !AG.draft }, project };
  };
  if (AG.draft) return slim([["⌘T", "terminal"], ["⌘2", "files"], ["esc", "back"]]);
  const mode: readonly Hint[] = hasModes() ? [["⇧Tab", "mode"]] : [];
  if (q && openModes()?.plan != null) return slim([[`⌘${ICON.ret}`, "approve"], ["⌘D", "keep planning"], ...mode, ["esc", "back"]]);
  if (q) return slim([[`⌘${ICON.ret}`, "allow"], ["⌘D", "deny"], ["esc", "back"]]);
  if (s && isNativeSession(s.id)) return slim([["esc", "parent session"]]);
  if (s && (followsOnly(s) || isStarting(s.id))) return slim([["esc", "back"]]);
  if (s?.status === "ended") return slim([[ICON.ret, "continue"], ["esc", "back"]]);

  if (turnRuns()) {
    const stop: readonly Hint[] = stoppingNow() ? [] : [["⌘.", "stop"]];
    return slim([...mode, ...stop, ["esc", "back"]]);
  }

  return slim([...mode, ["⌘U", "thoughts"], ["⌘Y", "commands"], ["esc", "back"]]);
}

/// Puts the session's key hints in place of those on screen.
export function redrawAgentHints() {
  const input = agentMessage.current, s = AG.page?.session;
  const placeholder = !s ? null : AG.att.length ? "Add a message (optional)" : s.status === "ended" ? `Message ${agentKind(s.agent).name} to continue` : `Message ${agentKind(s.agent).name}`;
  // Each delta redraws the hints; an unchanged write would still invalidate style.
  if (input && placeholder !== null && input.placeholder !== placeholder) input.placeholder = placeholder;
  updateSessionHints(agentHints());
}

/// A session's folder as the header shows it: the repository and its
/// worktree (`widget · fix-login`), else the folder's name.
function shortCwd(cwd: string) {
  const m = /^(.*?)\/(?:\.claude\/worktrees|\.worktrees)\/(.+?)\/?$/.exec(cwd);
  if (m) return `${folderName(m[1])} · ${m[2]}`;
  return folderName(cwd) || cwd;
}

/// The folder on this Mac the head's Open in button opens, for ⌘O.
let editorFolder: string | null = null;
let editorHead: { target: HTMLElement; key: string } | null = null;

/// ⌘O: the folder in the first editor, when this Mac has it.
export function openEditorKey() {
  if (editorFolder) void openInEditor(editorFolder);
}

/// Open in an editor, in the head, when this Mac has the folder `cwd` of
/// `machine`; `current` says the head still belongs to it.
export async function drawEditors(machine: string, cwd: string, current: () => boolean) {
  const target = sessionEditHost(), key = JSON.stringify([machine, cwd]);
  if (!target || (editorHead?.target === target && editorHead.key === key)) return;
  if (editorHead && editorHead.target !== target) removeSessionPortal(editorHead.target);
  const head = editorHead = { target, key };
  editorFolder = null;
  removeSessionPortal(target);
  const folder = await invoke<string | null>("session_folder_here", { machineId: machine, cwd })
    .catch(e => { console.warn("session folder not read", e); return null; });
  const draw = () => {
    if (!current() || editorHead !== head || sessionEditHost() !== target) return;
    editorFolder = folder;
    const editors = editorsFor(folder, draw);
    if (folder && editors.length) setSessionPortal(target, <ProjectEditors editors={editors} folder={folder} />);
    else removeSessionPortal(target);
  };
  draw();
}

// Titles, usage and checkout details can arrive after the frame first draws.
export function drawAgentHeading() {
  const page = AG.page;
  if (!page) return;

  const s = page.session;
  const usage = page.usage ? sessionUsage(page.usage) : null;
  if (!updateSessionHead({ ...headText(s), machine: s.machine_name, usage })) return;
  void drawEditors(s.machine_id, s.cwd, () => !!AG.page && agentKey(AG.page.session) === agentKey(s));
}
onPlaces(drawAgentHeading);

/// The head's title, the folder and worktree the session works in, and the custom agent it runs as.
export function headText(s: AgentRow) {
  const project = sessionProject(s), place = placeOf(s), name = agentKind(s.agent).name;
  return {
    title: s.title || name,
    folder: project?.label || (place ? placeRepo(place) : shortCwd(s.cwd)) || name,
    branch: place?.linked ? placeName(place) : "",
    persona: s.persona ?? null,
  };
}
