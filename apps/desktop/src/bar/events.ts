import { turnState } from "../chat/turn-state.ts";
import "./keys.ts";
import { linkedPath } from "../agentpane/media-state.ts";
import { approvalNotice, answeredTurns } from "../chat/approval-cards.ts";
import { openInWindow, renderChat } from "../chat/chat.ts";
import { watchRun } from "../chat/runs.ts";
import { loadChats } from "../chat/chats.ts";
import { copy } from "../ui/parts.ts";
import { bar, G, here, live, type LinkState, PANE, S } from "./core.ts";
import { drawGroup, type GroupApproval, loadGroup } from "../chat/groups.ts";
import { learnedHotkey, refreshHints } from "./hints.ts";
import { refreshSession } from "../chat/history.ts";
import { drawLink, loginFailed, renderLink, renderRevoked } from "./link.ts";
import { openViewer } from "../chat/media.ts";
import { loadModels, renderModel } from "../settings/model.ts";
import { renderSettings } from "../settings/settings.ts";
import { closeRerunMenu } from "../chat/rerun.ts";
import { forgetHeight } from "./size.ts";
import { show } from "./views.ts";
import { copyCode } from "../agents/turnrows.ts";
import { COPY_ICON, ICON } from "../ui/icons.tsx";
import { invoke, listen } from "../ui/tauri.ts";
import { ambient, paneShown } from "../ui/active.ts";

/// A button that says what it did for a moment, then goes back.
interface UndoButton extends HTMLElement {
  undo?: ReturnType<typeof setTimeout>;
}

// Only the Option double tap shows the bar without naming a view.
if (!PANE) listen<string | null>("bar-shown", e => { forgetHeight(); if (e.payload == null) learnedHotkey(); show(e.payload); });
listen<boolean>("signed-out", e => { S.signedOut = e.payload; });
listen<LinkState>("link-state", async e => {
  S.link = e.payload;
  // Signed out while the bar shows the assistant: it opens on coding agents instead.
  if (S.signedOut && S.view !== "settings" && S.view !== "login" && S.view !== "link") return void show(null);

  if (S.view === "link" && S.link === "linked") {
    clearInterval(S.timer);
    drawLink("linked");
    setTimeout(renderChat, 1800);
  } else if (S.view === "login" && S.link === "linked") {
    loadModels();
    renderChat();
  } else if (S.link === "revoked") {
    renderRevoked();
  }
});
listen<string>("login-failed", e => loginFailed(e.payload));
// Outside the list, a reload only refreshes the cache it opens with.
export const listIdle = () => S.view !== "chats" || (!S.chats.query && S.chats.mode === "list");
// While the bar is hidden a change only marks these; each runs once when it shows.
const reloadChats = ambient(() => { if (listIdle()) loadChats(""); }, 0, paneShown);
const reloadGroup = ambient(() => { if (S.view === "group" || S.view === "ginfo") loadGroup(); }, 0, paneShown);
listen<string>("group-changed", e => {
  if (G.id === e.payload && (S.view === "group" || S.view === "ginfo")) reloadGroup();
  else reloadChats();
});
listen<GroupApproval>("group-approval", e => {
  const a = e.payload;
  if (!G.approvals.has(a.group_id)) G.approvals.set(a.group_id, new Map());
  G.approvals.get(a.group_id)!.set(a.query_id, a);
  if (G.id === a.group_id && S.view === "group") drawGroup();
});
listen<string | null>("sessions-changed", e => {
  if (S.view === "chat" && !here()?.run && e.payload && e.payload === S.session) refreshSession(S.session).then(() => watchRun(e.payload));
  reloadChats();
});

/// Shows `done` in button `b` for a moment, then `back` again.
function flash(b: UndoButton, done: string, back: string) {
  b.innerHTML = done;
  clearTimeout(b.undo);
  b.undo = setTimeout(() => { b.innerHTML = back; }, 1200);
}

// Links in replies open in the browser. A product card's corner button
// copies its link instead.
bar.addEventListener("click", e => {
  const t = e.target as HTMLElement;
  if (t.closest(".dhints .st")) return S.link === "revoked" ? renderRevoked() : S.signedOut ? renderSettings() : renderLink();
  if (t.closest('[data-act="model"]')) return renderModel();
  if (t.closest('[data-act="openWindow"]')) return openInWindow(S.session);
  if (!t.closest(".dmenu")) closeRerunMenu();
  // The sanitized markup of replies and cards brings its own copy buttons: a
  // product card's copies its link, a code block's or a quote's its text.
  const link = t.closest<UndoButton>(".dprod-copy[data-copy]");

  if (link) {
    e.preventDefault();
    copy(link.dataset.copy ?? "");
    return flash(link, ICON.check, COPY_ICON);
  }

  const code = t.closest<HTMLElement>(".dcoderen.agent [data-copy]");
  if (code) return copyCode(code);

  const img = t.closest<HTMLImageElement>(".dcoderen.agent img");

  if (img && !img.closest("a, .dprods, .drecs")) {
    return openViewer([{ url: img.currentSrc || img.src, caption: img.alt }], 0);
  }

  const a = t.closest<HTMLAnchorElement>("a[href]");
  if (!a) return;
  e.preventDefault();
  // A link to a file, not a web address: the session's pane opens it.
  if (linkedPath(a.getAttribute("href")!)) return;
  invoke("open_url", { url: a.href }).catch(() => {});
});

// Product pictures show over their placeholder once loaded; a broken one
// leaves the placeholder. The product cards React draws handle their own.
const isProductPic = (t: EventTarget | null): t is Element => t instanceof Element && t.matches(".dprod-img img, .drec img") && !t.closest(".files > .dprods");
bar.addEventListener("load", e => { if (isProductPic(e.target)) e.target.classList.add("on"); }, true);
bar.addEventListener("error", e => { if (isProductPic(e.target)) e.target.remove(); }, true);

// An approval answered in another webview: its card here goes and the chat
// follows the run the answer resumed. The bar has usually settled it already
// from live mode's "answered" event, which comes first.
listen<{ session: string; query_id: string } | null>("approval-answered", e => {
  const { session, query_id } = e.payload ?? {};
  if (query_id) answeredTurns.mark(query_id, session ?? null);
  const en = live.get(session!), pending = en?.approval;
  if (!en || !pending || pending.queryId !== query_id) return;
  en.approval = null;
  approvalNotice(pending.turn, "Answered");
  turnState(pending.turn).done = false;
  if (!en.run) live.delete(session!);
  if (session !== S.session || S.view !== "chat") return;
  refreshHints();
  refreshSession(session).then(() => watchRun(session));
});
