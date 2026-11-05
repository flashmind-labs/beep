import { closeAgentNewMenus, renderAgentNew } from "../agents/agentnew.ts";
import { keyHidden } from "../settings/subscription.ts";
import { agentBack, agentKeys } from "../agents/agents.ts";
import { A, addServer, drawApps, drawMcpTools, renderApps } from "../settings/apps.ts";
import { attachedCount, unattach } from "../chat/attach.ts";
import { answer, newChat, openInWindow, popQueued, renderChat, send, sendQueued, stop, toWindow } from "../chat/chat.ts";
import { chatAction, chatMode, drawChats, openRow, renderChats } from "../chat/chats.ts";
import { $, $$, bar, G, here, PANE, queued, S, windowHooks } from "./core.ts";
import { composeMessage } from "../chat/compose.ts";
import { closeMention, drawMention, findMention, groupFormGo, insertMention, J, renderGroup, renderGroupInfo, renderGroupNew, sendGroup } from "../chat/groups.ts";
import { toggleKeys } from "./hints.ts";
import { openSession } from "../chat/history.ts";
import { LV, VN } from "./live.ts";
import { closeFilesMenu } from "../agentpane/files.ts";
import { viewerKeys } from "../chat/media.ts";
import { newTerminal } from "../agentpane/media.ts";
import { arrowKey, drawMemories, M, memKeys, renderMemories } from "../lists/memories.ts";
import { canPickModel, modelKeys, renderModel } from "../settings/model.ts";
import { closeRerunMenu } from "../chat/rerun.ts";
import { drawResearches, research, researchKeys, researchShortcut, RL } from "../research/researches.ts";
import { createRoutine, drawGrants, GR, grantKeys, R, renderGrants, renderRoutineNew, renderRoutines, routineAction, routineKeys } from "../lists/routines.ts";
import { renderSettings } from "../settings/settings.ts";
import { K, renderSkills, skillAction, skillKeys } from "../lists/skills.ts";
import { answerWork, waitingQuestion } from "./work.ts";
import { windowTakes } from "../ui/windowkeys.ts";
import { invoke } from "../ui/tauri.ts";

// ---------- Keys ----------

/// What a key's target is: a field, or any element the key came from.
type KeyTarget = HTMLElement & { value?: string };
const target = (e: KeyboardEvent) => e.target as KeyTarget;
const decision = (e: KeyboardEvent) => e.altKey ? "always" : e.shiftKey ? "session" : "allow";
/// Back on the open chat, or a new one.
const backToChat = () => S.session ? openSession(S.session) : renderChat();

export function back() {
  if (closeAgentNewMenus()) return;
  if (S.view === "agent" || S.view === "agentnew" || S.view === "agents") return agentBack();
  if (S.view === "chats") {
    if (S.chats.mode !== "list") return chatMode("list");
    if (S.chats.back) return renderGroup(S.chats.back);
    return backToChat();
  }
  if (S.view === "group" && G.mention) return closeMention();
  if (S.view === "memories" && M.mode !== "list") { M.mode = "list"; drawMemories(); return $("#mfind")?.focus(); }
  if (S.view === "routines" && R.mode !== "list") return routineAction("list");
  if (S.view === "grants" && GR.mode !== "list") { GR.mode = "list"; return drawGrants(); }
  if (S.view === "grants") return renderRoutines();
  if (S.view === "research" && RL.mode !== "list") { RL.mode = "list"; return drawResearches(); }
  if (S.view === "skills" && K.mode !== "list") return skillAction(K.mode === "edit" || K.mode === "vdelete" ? "view" : "list");
  if (S.view === "rnew") return renderRoutines();
  if (S.view === "appperms") return renderApps();
  if (S.view === "mcp" && A.asking) { A.asking = null; return drawMcpTools(); }
  if (S.view === "mcp") return renderApps();
  if (S.view === "apps" && A.adding) { A.adding = false; A.addError = ""; return drawApps(); }
  if (S.view === "apps" || S.view === "machines") return renderSettings();
  if (S.view === "model") return backToChat();
  if ((S.view === "memories" || S.view === "routines" || S.view === "skills" || S.view === "research" || S.view === "settings") && S.link === "linked") return backToChat();
  if (S.view === "ginfo" || S.view === "invite") return renderGroup(G.id!);
  if (S.view === "gnew") {
    if (J.mode === "join") return renderGroupNew("code");
    if (J.mode === "code") return renderGroupNew("new");
    return renderChats();
  }
  if (!PANE) invoke("hide_bar");
}

export function chatsKeys(e: KeyboardEvent, cmd: boolean) {
  const { list, mode } = S.chats, c = list[S.chats.sel];
  const key = e.key.toLowerCase();
  if (mode === "rename") {
    if (e.key === "Enter" && !e.isComposing) { e.preventDefault(); chatAction("rename-save"); }
    return;
  }
  if (mode === "delete" && e.key === "Enter") { e.preventDefault(); return chatAction("delete-yes"); }
  if (arrowKey(e, S.chats)) { S.chats.mode = "list"; return drawChats(); }
  if (e.key === "Tab" && c) { e.preventDefault(); return chatMode(mode === "actions" ? "list" : "actions"); }
  if (!PANE && cmd && key === "o" && c?.kind === "chat") { e.preventDefault(); return chatAction("window"); }
  if (cmd && key === "p" && c) { e.preventDefault(); return chatAction("pin"); }
  if (cmd && key === "r" && c?.kind === "chat") { e.preventDefault(); return chatAction("rename"); }
  if (cmd && e.key === "Backspace" && c?.kind === "chat") { e.preventDefault(); return chatAction("delete"); }
  if (e.key === "Enter" && c && mode === "list") { e.preventDefault(); return openRow(c); }
}

export function groupKeys(e: KeyboardEvent) {
  const t = target(e);
  if (t.id !== "gask") return;
  const mn = G.mention;
  if (mn && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
    e.preventDefault();
    mn.sel = (mn.sel + (e.key === "ArrowDown" ? 1 : mn.items.length - 1)) % mn.items.length;
    return drawMention();
  }
  if (mn && (e.key === "Enter" || e.key === "Tab") && !e.isComposing) { e.preventDefault(); return insertMention(mn.sel); }
  if (e.key === "Enter" && !e.shiftKey && !e.isComposing) { e.preventDefault(); return sendGroup(); }
  if (!PANE && e.key === "ArrowUp" && !t.value) { e.preventDefault(); return renderChats(); }
  if (e.key === "ArrowLeft" || e.key === "ArrowRight") setTimeout(findMention);
}

/// Keys while the bar shows live mode.
export function liveKeys(e: KeyboardEvent, cmd: boolean) {
  if (cmd && e.key === "/") { e.preventDefault(); invoke("live_sheet", { open: null }).catch(() => {}); return true; }
  if (e.key === "Escape") { e.preventDefault(); invoke("cancel_live").catch(() => {}); return true; }
  if (cmd && e.key.toLowerCase() === "q") return false;
  // The approval card in the chat answers to its usual keys, and text read
  // in the chat can be copied.
  const key = e.key.toLowerCase();
  if (cmd && (e.key === "Enter" || key === "d") && (here()?.approval || waitingQuestion(S.session))) return false;
  if (cmd && key === "c") return false;
  if (cmd || e.ctrlKey) { e.preventDefault(); return true; }
  if (e.key.length === 1 && !e.altKey && !e.isComposing && !LV.view?.title) {
    e.preventDefault();
    VN.typed += e.key;
    invoke("leave_live").catch(() => {});
    return true;
  }
  return false;
}

/// In the window, whether the pane is the view on screen; Home takes the keys
/// while it shows.
let paneMounted = false;
export function mountPane(on: boolean) { paneMounted = on; }
document.addEventListener("keydown", e => {
  const cmd = e.metaKey;
  if (PANE && !paneMounted) return;
  // A key meant for the sidebar (Enter on a row) is not the pane's.
  const t = target(e);
  if (PANE && !cmd && t !== document.body && !bar.contains(t)) return;
  // The window's palette and pages, Research among them, are the window's.
  if (PANE && cmd && windowTakes(e.key, S.view)) return;
  // ⌃` opens a terminal in the session's folder, in its pane.
  if (S.view === "agent" && e.ctrlKey && !cmd && e.key === "`") { e.preventDefault(); return newTerminal(); }
  // Keys typed into a terminal of the pane are the terminal's: Tab, Esc
  // and the arrows go to the shell.
  if (!cmd && t instanceof Element && t.closest(".tm-xterm")) return;
  if (e.key === "Escape" && closeFilesMenu()) { e.preventDefault(); return; }
  if ($("#viewer")) return viewerKeys(e, cmd);
  if (LV.on && liveKeys(e, cmd)) return;
  if (cmd && e.key === "/") { e.preventDefault(); return toggleKeys(); }
  if (e.key === "Escape" && LV.on) { e.preventDefault(); return invoke("cancel_live").catch(() => {}); }
  if (e.key === "Escape" && $("#keys")) { e.preventDefault(); return toggleKeys(); }
  if (e.key === "Escape" && closeRerunMenu()) { e.preventDefault(); return; }
  if (e.key === "Escape") { e.preventDefault(); return back(); }
  if (S.view === "link" || S.view === "login" || S.view === "revoked") return;
  if (agentKeys(e, cmd)) return;
  const key = e.key.toLowerCase();
  if (cmd && e.shiftKey && key === "a" && S.link !== "revoked") {
    e.preventDefault();
    return renderAgentNew(S.view === "chat" ? $<HTMLTextAreaElement>("#ask")?.value.trim() ?? "" : "", S.view === "agents" ? "agents" : "chat");
  }
  // These views live in the window now; before linking the bar keeps them.
  const linked = S.link === "linked";
  // Without a subscription the assistant's pages are not there, so their keys do nothing.
  if (cmd && keyHidden(key, linked)) { e.preventDefault(); return; }
  if (cmd && e.key === ",") { e.preventDefault(); return linked ? toWindow("settings") : renderSettings(); }
  if (cmd && key === "q") { e.preventDefault(); return invoke("quit"); }
  if (cmd && key === "m") { e.preventDefault(); return linked ? toWindow("memories") : renderMemories(); }
  if (cmd && key === "j") { e.preventDefault(); return linked ? toWindow("routines") : renderRoutines(); }
  if (cmd && key === "l") { e.preventDefault(); return linked ? toWindow("skills") : renderSkills(); }
  if (cmd && key === "r" && S.view !== "chats" && S.link === "linked") { e.preventDefault(); return researchShortcut(); }
  if (cmd && e.key === "Enter" && S.view === "rnew") { e.preventDefault(); return createRoutine(); }
  if (cmd && key === "n" && S.view === "routines") { e.preventDefault(); return renderRoutineNew(); }
  // The window picks the chat: the project's while one of its chats shows.
  if (cmd && key === "n") { e.preventDefault(); return S.link === "unlinked" ? renderAgentNew("", "chat") : windowHooks ? windowHooks.newChat() : newChat(); }
  if (cmd && key === "e" && S.view === "routines" && R.list.length) { e.preventDefault(); return routineAction("edit"); }
  if (cmd && key === "g" && S.view === "routines") { e.preventDefault(); return renderGrants(); }
  if (cmd && key === "g") { e.preventDefault(); J.error = ""; J.name = ""; return renderGroupNew("new"); }
  if (cmd && key === "k") { e.preventDefault(); return S.view === "chats" ? null : renderChats(); }
  if (cmd && key === "i" && (S.view === "group" || S.view === "invite")) { e.preventDefault(); return renderGroupInfo(); }
  if (cmd && e.key === ".") { e.preventDefault(); return stop(); }
  if (cmd && key === "e" && canPickModel()) { e.preventDefault(); return S.view === "model" ? back() : renderModel(); }
  if (S.view === "model") return modelKeys(e);
  if (S.view === "chat") {
    if (cmd && e.key === "Enter" && here()?.approval) { e.preventDefault(); return answer(decision(e)); }
    if (cmd && key === "d" && here()?.approval) { e.preventDefault(); return answer("deny"); }
    const asked = !here()?.approval && waitingQuestion(S.session);
    if (cmd && e.key === "Enter" && asked) { e.preventDefault(); return answerWork(S.session, asked, decision(e)); }
    if (cmd && key === "d" && asked) { e.preventDefault(); return answerWork(S.session, asked, "deny"); }
    if (cmd && e.key === "Enter" && queued.get(S.session)?.held && !here()) { e.preventDefault(); return sendQueued(S.session); }
    if (!PANE && cmd && key === "o") { e.preventDefault(); return openInWindow(S.session); }
    const ask = t.id === "ask" ? t.value! : null;
    if (cmd && e.shiftKey && e.key === "Enter" && ask != null && S.link === "linked") { e.preventDefault(); return research(composeMessage(ask)); }
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing && ask != null) { e.preventDefault(); return send(composeMessage(ask)); }
    if (e.key === "ArrowUp" && ask === "" && popQueued()) { e.preventDefault(); return; }
    if (!PANE && e.key === "ArrowUp" && ask === "") { e.preventDefault(); return renderChats(); }
    if (e.key === "Backspace" && !cmd && ask === "" && attachedCount()) { e.preventDefault(); return unattach(attachedCount() - 1); }
  }
  if (S.view === "chats") return chatsKeys(e, cmd);
  if (S.view === "memories") return memKeys(e, cmd);
  if (S.view === "routines") return routineKeys(e, cmd);
  if (S.view === "grants") return grantKeys(e, cmd);
  if (S.view === "research") return researchKeys(e);
  if (S.view === "skills") return skillKeys(e, cmd);
  if (S.view === "group") return groupKeys(e);
  if (S.view === "gnew" && e.key === "Enter" && !e.isComposing && t.tagName === "INPUT") { e.preventDefault(); return groupFormGo(); }
  if (S.view === "apps" && e.key === "Enter" && !e.isComposing && (t.id === "mname" || t.id === "murl")) {
    e.preventDefault();
    const url = $$<HTMLInputElement>("#murl");
    return t.id === "mname" && !url.value.trim() ? url.focus() : addServer();
  }
});

