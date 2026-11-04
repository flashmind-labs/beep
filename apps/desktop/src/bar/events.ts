import "./keys.ts";
import { chartClick } from "../chat/charts.ts";
import { answeredTurns, openInWindow, renderChat, watchRun } from "../chat/chat.ts";
import { copy, loadChats } from "../chat/chats.ts";
import { bar, COPY_ICON, G, h, here, ICON, invoke, listen, live, type LinkState, PANE, S, type Turn } from "./core.ts";
import { drawGroup, type GroupApproval, loadGroup } from "../chat/groups.ts";
import { learnedHotkey, refreshHints } from "./hints.ts";
import { refreshSession } from "../chat/history.ts";
import { drawLink, renderLink, renderRevoked } from "./link.ts";
import { openViewer } from "../chat/media.ts";
import { modelClick, renderModel } from "../settings/model.ts";
import { closeRerunMenu, rerun, rerunClick, rerunMenu } from "../chat/rerun.ts";
import { routeClick } from "../chat/routes.ts";
import { forgetHeight } from "./size.ts";
import { show } from "./views.ts";

/// A button that says what it did for a moment, then goes back.
interface UndoButton extends HTMLElement {
  undo?: ReturnType<typeof setTimeout>;
}

// Only the Option double tap shows the bar without naming a view.
if (!PANE) listen<string | null>("bar-shown", e => { forgetHeight(); if (e.payload == null) learnedHotkey(); show(e.payload); });
listen<LinkState>("link-state", async e => {
  S.link = e.payload;
  if (S.view === "link" && S.link === "linked") {
    clearInterval(S.timer);
    drawLink("linked");
    setTimeout(renderChat, 1800);
  } else if (S.link === "revoked") {
    renderRevoked();
  }
});
// Outside the list, a reload only refreshes the cache it opens with.
export const listIdle = () => S.view !== "chats" || (!S.chats.query && S.chats.mode === "list");
listen<string>("group-changed", e => {
  if (G.id === e.payload && (S.view === "group" || S.view === "ginfo")) loadGroup();
  else if (listIdle()) loadChats("");
});
listen<GroupApproval>("group-approval", e => {
  const a = e.payload;
  if (!G.approvals.has(a.group_id)) G.approvals.set(a.group_id, new Map());
  G.approvals.get(a.group_id)!.set(a.query_id, a);
  if (G.id === a.group_id && S.view === "group") drawGroup();
});
listen<string | null>("sessions-changed", e => {
  if (S.view === "chat" && !here()?.run && e.payload && e.payload === S.session) refreshSession(S.session).then(() => watchRun(e.payload));
  if (listIdle()) loadChats("");
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
  if (t.closest(".dhints .st")) return S.link === "revoked" ? renderRevoked() : renderLink();
  if (t.closest('[data-act="model"]')) return renderModel();
  if (t.closest('[data-act="openWindow"]')) return openInWindow(S.session);
  if (S.view === "model") return modelClick(e);
  if (rerunClick(e)) return;
  if (t.closest(".dchart [data-series], .dchart [data-range]")) return void chartClick(e);
  if (t.closest(".droute [data-route], .droute [data-maps], .droute [data-stops]")) return void routeClick(e);
  const retry = t.closest<HTMLElement>('[data-act="retry"]');
  if (retry) return void rerun(retry.closest<Turn>(".turn")!, null, null);
  const menuButton = t.closest<HTMLElement>('[data-act="rerunMenu"]');
  if (menuButton) return rerunMenu(menuButton);
  if (!t.closest(".dmenu")) closeRerunMenu();
  const copyReply = t.closest<UndoButton>('[data-act="copyReply"]');
  if (copyReply) {
    copy(copyReply.closest(".turn")!.querySelector<HTMLElement>(".reply")?.innerText.trim() ?? "");
    return flash(copyReply, ICON.check + "Copied", COPY_ICON + "Copy");
  }
  const c = t.closest<UndoButton>("[data-copy]");
  if (c) {
    e.preventDefault();
    copy(c.dataset.copy!);
    return flash(c, ICON.check, COPY_ICON);
  }
  const img = t.closest<HTMLImageElement>(".reply img");
  if (img && !img.closest("a, .dprods, .drecs")) {
    return openViewer([{ url: img.currentSrc || img.src, caption: img.alt }], 0);
  }
  const a = t.closest<HTMLAnchorElement>("a[href]");
  if (!a) return;
  e.preventDefault();
  invoke("open_url", { url: a.href }).catch(() => {});
});

// Product pictures show over their placeholder once loaded; a broken one
// leaves the placeholder.
const isProductPic = (t: EventTarget | null): t is Element => t instanceof Element && t.matches(".dprod-img img, .drec img");
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
  pending.card.replaceWith(h('<div class="dmuted">Answered</div>'));
  pending.turn.done = false;
  if (!en.run) live.delete(session!);
  if (session !== S.session || S.view !== "chat") return;
  refreshHints();
  refreshSession(session).then(() => watchRun(session));
});
