// What a click in the open session's log and pane does.
import { S } from "../bar/core.ts";
import { shell } from "../center/window-shell.tsx";
import { openFileViewer, openViewer } from "../chat/media.tsx";
import { linkedPath, resolvePath } from "../agentpane/media-state.ts";
import { openLinked, openPath, openPicture, showArtifact, showRunning, showSubagents } from "../agentpane/media.ts";
import { paneSession } from "../agentpane/pane-state.ts";
import { atBottom } from "./logscroll.ts";
import { copyCode } from "./turnrows.ts";
import { AG, openOf, where } from "./agents-state.ts";
import { fullPic } from "./images.tsx";
import { childRow } from "./nesting-state.ts";
import { parentSession, keepNativeSession, nextSessions, sessions } from "./sessions.ts";
import { turnsOf, clickedRow, drawAgentBody, jumpToLatest, closeSub } from "./logview.tsx";
import { toggleModelPicker, closeModelPicker, retryWithModel } from "./modelpill.tsx";
import { cycleMode } from "./modepill.tsx";
import { answerAgentQuestion, interruptAgent } from "./messages.ts";
import { invoke } from "../ui/tauri.ts";
import { readError } from "../agentpane/read-file.ts";
import { imageLines, parseTool, subagent } from "./agentlog.ts";
import { continuedFrom, continuedIn } from "./continue-state.ts";
import { nativeSession } from "./native-session-state.ts";
import { isOpen, resumeFollow, wantSession, setNote } from "./open-state.ts";
import { agentMessage, sessionBody } from "./session-frame-state.ts";

/// How long a card waits for the machine to list a child that is not in the list yet.
const KID_WAIT_MS = 3000;

/// A chat on the Assistant page without a pane beside it opens its files over
/// the window, read from its machine. False elsewhere: the pane shows them.
function openOver(path: string) {
  const s = AG.page?.session;
  if (!s || !shell.getState().assistant || paneSession()?.id === s.id) return false;
  openFileViewer(s.machine_id, s.id, s.cwd, resolvePath(s.cwd, path));
  return true;
}

/// Opens the conversation of the subagent whose call is entry `at`; false when it has none to open.
function openNative(at: number) {
  const entry = AG.page ? AG.page.entries[at - AG.page.earlier] : null;
  if (!AG.page || !entry) return false;

  const { title, status, output } = parseTool(imageLines(entry.text).text);
  const child = subagent(output);
  const row = child?.session && nativeSession(AG.page.session, child.session, title.split("\n")[0], status);
  if (!row) return false;

  keepNativeSession(row, AG.page.session);
  wantSession(row, AG.back);
  return true;
}

export function agentClick(e: MouseEvent) {
  if (S.view !== "agent") return;
  const target = e.target as HTMLElement, body = sessionBody();
  /// The element `selector` finds from the click when it is in the log, or also in the pane.
  const inLog = <T extends HTMLElement = HTMLElement>(selector: string) => { const el = target.closest<T>(selector); return el && body?.contains(el) ? el : null; };
  const inLogOrPane = <T extends HTMLElement = HTMLElement>(selector: string) => { const el = target.closest<T>(selector); return el && (body?.contains(el) || el.closest(".agpane, .ag-steps")) ? el : null; };
  const opt = target.closest<HTMLElement>("[data-agopt]");
  if (opt) return answerAgentQuestion(Number(opt.dataset.agopt), opt.dataset.agquestion ?? "");
  if (target.closest("[data-agcancel]")) return void interruptAgent();
  if (target.closest("#agmodelb")) return toggleModelPicker();
  if (target.closest("[data-agretrymodel]")) return retryWithModel();
  if (target.closest("#agmodeb")) { cycleMode(); agentMessage.current?.focus(); return; }
  if (AG.modelOpen && !target.closest(".agc-pick, [data-agtask]")) closeModelPicker();
  const closing = target.closest<HTMLElement>(".agpane [data-subclose]");
  if (closing) return closeSub(Number(closing.dataset.subclose));
  const stopping = inLogOrPane<HTMLElement>("[data-kidstop]");
  if (stopping) return void stopKid(stopping);
  const prompt = inLog<HTMLElement>("[data-prompt]");

  if (prompt) {
    const at = Number(prompt.dataset.prompt);
    AG.steps.set(at, prompt.getAttribute("aria-expanded") !== "true");
    return drawAgentBody(clickedRow(prompt));
  }

  const summary = target.closest<HTMLElement>("[data-subsummary]");
  if (summary) { const at = Number(summary.dataset.subsummary); AG.steps.set(at, !AG.steps.get(at)); drawAgentBody(); return; }
  const sub = inLogOrPane<HTMLElement>("[data-native]");
  if (sub) { openNative(Number(sub.dataset.native)); return; }
  // The line of what a replied turn waits for opens its oldest subagent, or the Running tab.
  const wait = inLog<HTMLElement>("[data-wait]");

  if (wait) {
    if (wait.dataset.wait === "running") showRunning();
    else if (!openNative(Number(wait.dataset.wait))) showSubagents();
    return;
  }

  // An agent another agent started opens as its own session.
  const kid = inLogOrPane<HTMLElement>("[data-kid]");
  if (kid) { void openKid(kid.dataset.kid!, kid.dataset.key); return; }
  if (target.closest("#agup, #agup2")) return openParent();
  // The line that links a chat to the one it went on from, or to the one that went on from it.
  const continued = inLog<HTMLElement>("[data-continued]");
  if (continued) return openContinued(continued.dataset.continued === "in");
  // "Show all N lines" opens the whole output in a box that scrolls.
  const all = inLog<HTMLElement>("[data-all]");

  if (all) {
    const at = Number(all.dataset.all);
    if (!AG.all.delete(at)) AG.all.add(at);
    return drawAgentBody(clickedRow(all));
  }

  if (inLog("pre.out.all")) return;
  // A run of calls opens and folds where it is, like a thought.
  const runBtn = inLog<HTMLElement>("[data-run]");
  if (runBtn && AG.open) return toggleRow(turnsOf(AG.open.session).runs, Number(runBtn.dataset.run));
  // The files a turn made fold and open, like a run of calls.
  const madeHead = inLog<HTMLElement>("[data-madeat]");
  if (madeHead && AG.open) return toggleRow(turnsOf(AG.open.session).made, Number(madeHead.dataset.madeat));

  // A file a turn made opens in the Artifacts tab, or else in its own tab.
  const madeFile = inLog<HTMLElement>("[data-made]");

  if (madeFile) {
    if (!showArtifact(madeFile.dataset.made!)) openLinked({ path: madeFile.dataset.made!, line: null });
    return;
  }

  const copy = inLog<HTMLElement>("[data-copy]");
  if (copy) return copyCode(copy);
  if (target.closest("#agjump")) { jumpToLatest(); return; }
  // A thought opens and folds where it is; only its row draws again.
  const thought = inLog<HTMLElement>("[data-thought]");
  if (thought && AG.open) return toggleRow(turnsOf(AG.open.session).thoughts, Number(thought.dataset.thought));

  // The heading toggles the steps; the bottom chevron returns to it.
  const turn = inLog<HTMLElement>("[data-turn]");

  if (turn && AG.open) {
    const opened = turnsOf(AG.open.session).opened, key = Number(turn.dataset.turn);
    const collapse = turn.hasAttribute("data-collapse");
    const heading = () => body?.querySelector<HTMLButtonElement>(`.hd[data-turn="${key}"]`);
    // Bring the heading into view before the shorter turn is virtualized.
    if (collapse) heading()?.scrollIntoView({ block: "nearest" });
    if (!opened.delete(key) && !collapse) opened.add(key);
    drawAgentBody(clickedRow(turn));

    if (collapse) {
      const head = heading();
      head?.focus({ preventScroll: true });
      head?.scrollIntoView({ block: "nearest" });
    }

    return;
  }

  // A file a step names opens in the media pane.
  const step = inLog<HTMLElement>("[data-media]");
  if (step) { if (!openOver(step.dataset.media!)) openPath(step.dataset.media!); return; }
  const attachment = inLog<HTMLElement>("[data-attachment-path]");
  if (attachment?.dataset.attachmentPath) { if (!openOver(attachment.dataset.attachmentPath)) openPath(attachment.dataset.attachmentPath); return; }

  // A markdown link to a file opens it in the pane; bar/events.ts sends web links out.
  const fileLink = inLog<HTMLAnchorElement>("a[href]");
  const linkedFile = fileLink && linkedPath(fileLink.getAttribute("href")!);
  if (linkedFile) { if (!openOver(linkedFile.path)) openLinked(linkedFile); return; }
  // Inline code that names a file opens it the same way.
  const codeRef = inLog("code.fref[data-file]");
  const codeFile = codeRef && linkedPath(codeRef.dataset.file ?? "");
  if (codeFile) { if (!openOver(codeFile.path)) openLinked(codeFile); return; }
  // A file the reply names opens its picture, as its thumbnail does, or
  // else the file itself from the machine, in the media pane.
  const link = inLog<HTMLElement>("a.imgl");
  const linked = link?.dataset.img != null ? link.closest(".dcoderen")?.querySelectorAll<HTMLImageElement>(":scope > .pics img.pic, :scope > [data-tool-content] > .pics img.pic")[Number(link.dataset.img)] : null;
  if (link && !linked?.getAttribute("src")) { if (link.dataset.path && !openOver(link.dataset.path)) openPath(link.dataset.path); return; }
  // The agent's pictures open in the media pane, the user's own full size.
  // One still loading opens once it is here. The tile may be a thumbnail, so
  // what opens is read whole.
  const pic = linked ?? inLog<HTMLImageElement>("img.pic");

  if (pic) {
    const o = AG.open;
    if (!o || !pic.getAttribute("src")) return;
    const name = (link ? link.textContent : null) || pic.dataset.name || pic.alt;
    const unread = () => { setNote("Could not read the whole picture."); drawAgentBody(); };

    if (!pic.closest(".dcoderen.user")) {
      fullPic(o, pic).then(url => { if (isOpen(o)) url ? openPicture(url, name) : unread(); }).catch(() => { if (isOpen(o)) unread(); });
      return;
    }

    const all = [...pic.parentElement!.querySelectorAll<HTMLImageElement>("img.pic[src]")];
    Promise.all(all.map(i => fullPic(o, i))).then(urls => {
      if (!isOpen(o)) return;
      // Each opens by its data URL, which can be saved or copied.
      const read = all.flatMap((i, at) => { const url = urls[at]; return url ? [{ img: i, url }] : []; });
      const at = read.findIndex(r => r.img === pic);
      if (at < 0) return unread();
      openViewer(read.map(({ img: i, url }) => ({ url, name: i.dataset.name || i.alt, caption: "", saves: url.startsWith("data:") })), at);
    }).catch(() => { if (isOpen(o)) unread(); });
    return;
  }

  // An Artifact call opens what it published in the Artifacts tab.
  const art = inLog<HTMLElement>("[data-artifact]");
  if (art && !getSelection()!.toString() && showArtifact(art.dataset.artifact!)) return;
  const tool = inLog<HTMLElement>(".dcoderen.tool[data-at]");

  if (tool && !getSelection()!.toString() && !target.closest(".out, .ddiff, .cmd, .call-details")) {
    const at = Number(tool.dataset.at);
    AG.steps.set(at, !tool.classList.contains("open"));
    return drawAgentBody(clickedRow(tool));
  }

  // A pasted block opens by itself; remember it so a redraw keeps it open.
  const paste = inLog<HTMLElement>(".dcoderen.user[data-at] details.paste > summary");

  if (paste) {
    const key = `${paste.closest<HTMLElement>("[data-at]")!.dataset.at}:${paste.parentElement!.dataset.paste}`;
    if (!AG.opened.delete(key)) AG.opened.add(key);
    // Once it opened, the next redraw follows the bottom only if it is still in view.
    requestAnimationFrame(() => { const b = sessionBody(); if (b) b.stuck = atBottom(b); });
    return;
  }

  if (target.closest("#agresume")) return resumeFollow();
}

/// Opens or folds the row at `at`, which stays where it is on screen.
function toggleRow(opened: Set<number>, at: number) {
  if (!opened.delete(at)) opened.add(at);
  drawAgentBody(`[data-row="${at}"]`);
}

/// Opens the agent a card names. A child started after the list was pushed is not in it yet, so a miss waits
/// for the lists that follow, until the row is in one or `KID_WAIT_MS` passes (a retitle or another session's
/// push does not end it); one still missing is gone, which the log says.
async function kidSession(id: string, key?: string) {
  const row = childRow(sessions, id, key);
  if (row) return row;
  const until = Date.now() + KID_WAIT_MS;
  let found: ReturnType<typeof childRow>;

  while (!found && Date.now() < until) {
    await nextSessions(until - Date.now());
    found = childRow(sessions, id, key);
  }
  if (!found) { setNote("That agent's session is no longer on its machine."); drawAgentBody(); }
  return found;
}

async function openKid(id: string, key?: string) {
  const row = await kidSession(id, key);
  if (row) wantSession(row, AG.back);
}

/// Stop on a running child's row: stops its turn, as Stop in its own log does.
async function stopKid(x: HTMLElement) {
  const kid = x.closest<HTMLElement>("[data-kid]");
  const row = kid && await kidSession(kid.dataset.kid!, kid.dataset.key);
  if (!row) return;

  try {
    await invoke("agent_steer", { ...where(openOf(row)), steer: "cancel" });
  } catch (e) {
    setNote(readError(e));
    drawAgentBody();
  }
}

/// Opens the session the open one went on from, or with `onward` the one that went on from it.
function openContinued(onward: boolean) {
  const s = AG.open && AG.page?.session;
  const row = s && (onward ? continuedIn(s, sessions) : continuedFrom(s, sessions));
  if (row) wantSession(row, AG.back);
}

/// Opens the session that started the open one.
export function openParent() {
  const row = AG.open && AG.page && parentSession(AG.page.session);
  if (row) wantSession(row, AG.back);
}

/// Tool rows also open from the keyboard; details stay selectable.
export function toolKey(event: KeyboardEvent) {
  const target = event.target;
  if (!(target instanceof HTMLElement) || !target.matches('.dcoderen.tool[data-at][role="button"], code.fref[data-file]') || !sessionBody()?.contains(target) || !["Enter", " "].includes(event.key)) return;
  event.preventDefault();
  target.click();
}
