import { renderAgentNew } from "./agentnew.ts";
import { agentBack, agentKeys } from "./agents.ts";
import { A, addServer, drawApps, drawMcpTools, renderApps } from "./apps.ts";
import { attachedCount, unattach } from "./attach.ts";
import { answer, newChat, openInWindow, renderChat, send, sendQueued, toWindow } from "./chat.ts";
import { chatAction, chatMode, drawChats, openRow, renderChats } from "./chats.ts";
import { $, bar, G, here, invoke, PANE, queued, S, windowHooks } from "./core.ts";
import { closeMention, drawMention, findMention, groupFormGo, insertMention, J, renderGroup, renderGroupInfo, renderGroupNew, sendGroup } from "./groups.ts";
import { toggleKeys } from "./hints.ts";
import { openSession } from "./history.ts";
import { LV, VN } from "./live.ts";
import { viewerKeys } from "./media.ts";
import { drawMemories, M, memKeys, renderMemories } from "./memories.ts";
import { canPickModel, modelKeys, renderModel } from "./model.ts";
import { closeRerunMenu } from "./rerun.ts";
import { drawResearches, research, researchKeys, researchShortcut, RL } from "./researches.ts";
import { createRoutine, drawGrants, GR, grantKeys, R, renderGrants, renderRoutineNew, renderRoutines, routineAction, routineKeys } from "./routines.ts";
import { renderSettings } from "./settings.ts";
import { K, renderSkills, skillAction, skillKeys } from "./skills.ts";
import { answerWork, waitingQuestion } from "./work.ts";

// ---------- Keys ----------

export function back() {
  if (S.view === "agent" || S.view === "agentnew" || S.view === "agents") return agentBack();
  if (S.view === "chats") {
    if (S.chats.mode !== "list") return chatMode("list");
    if (S.chats.back) return renderGroup(S.chats.back);
    return S.session ? openSession(S.session) : renderChat();
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
  if (S.view === "model") return S.session ? openSession(S.session) : renderChat();
  if ((S.view === "memories" || S.view === "routines" || S.view === "skills" || S.view === "research" || S.view === "settings") && S.link === "linked") return S.session ? openSession(S.session) : renderChat();
  if (S.view === "ginfo" || S.view === "invite") return renderGroup(G.id);
  if (S.view === "gnew") {
    if (J.mode === "join") return renderGroupNew("code");
    if (J.mode === "code") return renderGroupNew("new");
    return renderChats();
  }
  if (!PANE) invoke("hide_bar");
}

export function chatsKeys(e, cmd) {
  const { list, mode } = S.chats, c = list[S.chats.sel];
  const key = e.key.toLowerCase();
  if (mode === "rename") {
    if (e.key === "Enter" && !e.isComposing) { e.preventDefault(); chatAction("rename-save"); }
    return;
  }
  if (mode === "delete" && e.key === "Enter") { e.preventDefault(); return chatAction("delete-yes"); }
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    S.chats.sel = Math.max(0, Math.min(list.length - 1, S.chats.sel + (e.key === "ArrowDown" ? 1 : -1)));
    S.chats.mode = "list";
    drawChats();
  } else if (e.key === "Tab" && c) {
    e.preventDefault();
    chatMode(mode === "actions" ? "list" : "actions");
  } else if (!PANE && cmd && key === "o" && c?.kind === "chat") {
    e.preventDefault();
    chatAction("window");
  } else if (cmd && key === "p" && c) {
    e.preventDefault();
    chatAction("pin");
  } else if (cmd && key === "r" && c?.kind === "chat") {
    e.preventDefault();
    chatAction("rename");
  } else if (cmd && e.key === "Backspace" && c?.kind === "chat") {
    e.preventDefault();
    chatAction("delete");
  } else if (e.key === "Enter" && c && mode === "list") {
    e.preventDefault();
    openRow(c);
  }
}

export function groupKeys(e, cmd) {
  if (e.target.id !== "gask") return;
  const mn = G.mention;
  if (mn && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
    e.preventDefault();
    mn.sel = (mn.sel + (e.key === "ArrowDown" ? 1 : mn.items.length - 1)) % mn.items.length;
    return drawMention();
  }
  if (mn && (e.key === "Enter" || e.key === "Tab") && !e.isComposing) { e.preventDefault(); return insertMention(mn.sel); }
  if (e.key === "Enter" && !e.shiftKey && !e.isComposing) { e.preventDefault(); return sendGroup(); }
  if (!PANE && e.key === "ArrowUp" && !e.target.value) { e.preventDefault(); return renderChats(); }
  if (e.key === "ArrowLeft" || e.key === "ArrowRight") setTimeout(findMention);
}

/// Keys while the bar shows live mode. ⌘/ opens what to say over it and esc
/// closes that first (Rust keeps the sheet with the view). Keys that would
/// open another view do nothing; a typed character leaves live mode, keeping
/// the words, and goes in the prompt after them. True when the key is taken.
export function liveKeys(e, cmd) {
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
/// Keys the window keeps for itself: its palette and its views.
export const WINDOW_KEYS = new Set(["k", "m", "j", "l", ","]);

document.addEventListener("keydown", e => {
  const cmd = e.metaKey;
  if (PANE && !paneMounted) return;
  // A key meant for the sidebar (Enter on a row) is not the pane's.
  if (PANE && !cmd && e.target !== document.body && !bar.contains(e.target)) return;
  if (PANE && cmd && WINDOW_KEYS.has(e.key.toLowerCase())) return;
  if ($("#viewer")) return viewerKeys(e, cmd);
  if (LV.on && liveKeys(e, cmd)) return;
  if (cmd && e.key === "/") { e.preventDefault(); return toggleKeys(); }
  if (e.key === "Escape" && LV.on) { e.preventDefault(); return invoke("cancel_live").catch(() => {}); }
  if (e.key === "Escape" && $("#keys")) { e.preventDefault(); return toggleKeys(); }
  if (e.key === "Escape" && closeRerunMenu()) { e.preventDefault(); return; }
  if (e.key === "Escape") {
    e.preventDefault();
    return back();
  }
  if (S.view === "link" || S.view === "revoked") return;
  if (agentKeys(e, cmd)) return;
  const key = e.key.toLowerCase();
  if (cmd && e.shiftKey && key === "a" && S.link === "linked") {
    e.preventDefault();
    return renderAgentNew(S.view === "chat" ? $("#ask")?.value.trim() ?? "" : "", S.view === "agents" ? "agents" : "chat");
  }
  // These views live in the window now; before linking the bar keeps them.
  const linked = S.link === "linked";
  if (cmd && e.key === ",") { e.preventDefault(); return linked ? toWindow("settings") : renderSettings(); }
  if (cmd && key === "q") { e.preventDefault(); return invoke("quit"); }
  if (cmd && key === "m") { e.preventDefault(); return linked ? toWindow("memories") : renderMemories(); }
  if (cmd && key === "j") { e.preventDefault(); return linked ? toWindow("routines") : renderRoutines(); }
  if (cmd && key === "l") { e.preventDefault(); return linked ? toWindow("skills") : renderSkills(); }
  if (cmd && key === "r" && S.view !== "chats") { e.preventDefault(); return researchShortcut(); }
  if (cmd && e.key === "Enter" && S.view === "rnew") { e.preventDefault(); return createRoutine(); }
  if (cmd && key === "n" && S.view === "routines") { e.preventDefault(); return renderRoutineNew(); }
  // The window picks the chat: the project's while one of its chats shows.
  if (cmd && key === "n") { e.preventDefault(); return windowHooks ? windowHooks.newChat() : newChat(); }
  if (cmd && key === "e" && S.view === "routines" && R.list.length) { e.preventDefault(); return routineAction("edit"); }
  if (cmd && key === "g" && S.view === "routines") { e.preventDefault(); return renderGrants(); }
  if (cmd && key === "g") { e.preventDefault(); J.error = ""; J.name = ""; return renderGroupNew("new"); }
  if (cmd && key === "k") { e.preventDefault(); return S.view === "chats" ? null : renderChats(); }
  if (cmd && key === "i" && (S.view === "group" || S.view === "invite")) { e.preventDefault(); return renderGroupInfo(); }
  if (cmd && e.key === ".") { e.preventDefault(); return stop(); }
  if (cmd && key === "e" && canPickModel()) { e.preventDefault(); return S.view === "model" ? back() : renderModel(); }
  if (S.view === "model") return modelKeys(e);
  if (S.view === "chat") {
    if (cmd && e.key === "Enter" && here()?.approval) { e.preventDefault(); return answer(e.altKey ? "always" : e.shiftKey ? "session" : "allow"); }
    if (cmd && key === "d" && here()?.approval) { e.preventDefault(); return answer("deny"); }
    const asked = !here()?.approval && S.view === "chat" && waitingQuestion(S.session);
    if (cmd && e.key === "Enter" && asked) { e.preventDefault(); return answerWork(S.session, asked, e.altKey ? "always" : e.shiftKey ? "session" : "allow"); }
    if (cmd && key === "d" && asked) { e.preventDefault(); return answerWork(S.session, asked, "deny"); }
    if (cmd && e.key === "Enter" && queued.get(S.session)?.held && !here()) { e.preventDefault(); return sendQueued(S.session); }
    if (!PANE && cmd && key === "o") { e.preventDefault(); return openInWindow(S.session); }
    if (cmd && e.shiftKey && e.key === "Enter" && e.target.id === "ask") { e.preventDefault(); return research(e.target.value); }
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing && e.target.id === "ask") { e.preventDefault(); return send(e.target.value); }
    if (!PANE && e.key === "ArrowUp" && e.target.id === "ask" && !e.target.value) { e.preventDefault(); return renderChats(); }
    if (e.key === "Backspace" && !cmd && e.target.id === "ask" && !e.target.value && attachedCount()) { e.preventDefault(); return unattach(attachedCount() - 1); }
  }
  if (S.view === "chats") return chatsKeys(e, cmd);
  if (S.view === "memories") return memKeys(e, cmd);
  if (S.view === "routines") return routineKeys(e, cmd);
  if (S.view === "grants") return grantKeys(e, cmd);
  if (S.view === "research") return researchKeys(e);
  if (S.view === "skills") return skillKeys(e, cmd);
  if (S.view === "group") return groupKeys(e, cmd);
  if (S.view === "gnew" && e.key === "Enter" && !e.isComposing && e.target.tagName === "INPUT") { e.preventDefault(); return groupFormGo(); }
  if (S.view === "apps" && e.key === "Enter" && !e.isComposing && (e.target.id === "mname" || e.target.id === "murl")) {
    e.preventDefault();
    return e.target.id === "mname" && !$("#murl").value.trim() ? $("#murl").focus() : addServer();
  }
});

