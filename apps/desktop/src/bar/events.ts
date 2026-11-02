import "./keys.ts";
import { chartClick } from "./charts.ts";
import { openInWindow, renderChat, watchRun } from "./chat.ts";
import { copy, loadChats } from "./chats.ts";
import { bar, COPY_ICON, G, h, here, ICON, invoke, listen, live, PANE, S } from "./core.ts";
import { drawGroup, loadGroup } from "./groups.ts";
import { learnedHotkey, refreshHints } from "./hints.ts";
import { refreshSession } from "./history.ts";
import { drawLink, renderLink, renderRevoked } from "./link.ts";
import { openViewer } from "./media.ts";
import { modelClick, renderModel } from "./model.ts";
import { closeRerunMenu, rerunClick, rerunMenu } from "./rerun.ts";
import { routeClick } from "./routes.ts";
import { forgetHeight } from "./size.ts";
import { show } from "./views.ts";

// Only the Option double tap shows the bar without naming a view.
if (!PANE) listen("bar-shown", e => { forgetHeight(); if (e.payload == null) learnedHotkey(); show(e.payload); });
listen("link-state", async e => {
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
listen("group-changed", e => {
  if (G.id === e.payload && (S.view === "group" || S.view === "ginfo")) loadGroup();
  else if (listIdle()) loadChats("");
});
listen("group-approval", e => {
  const a = e.payload;
  if (!G.approvals.has(a.group_id)) G.approvals.set(a.group_id, new Map());
  G.approvals.get(a.group_id).set(a.query_id, a);
  if (G.id === a.group_id && S.view === "group") drawGroup();
});
listen("sessions-changed", e => {
  if (S.view === "chat" && !here()?.run && e.payload && e.payload === S.session) refreshSession(S.session).then(() => watchRun(e.payload));
  if (listIdle()) loadChats("");
});

// Links in replies open in the browser. A product card's corner button
// copies its link instead.
bar.addEventListener("click", e => {
  if (e.target.closest(".dhints .st")) return S.link === "revoked" ? renderRevoked() : renderLink();
  if (e.target.closest('[data-act="model"]')) return renderModel();
  if (e.target.closest('[data-act="openWindow"]')) return openInWindow(S.session);
  if (S.view === "model") return modelClick(e);
  if (rerunClick(e)) return;
  if (e.target.closest(".dchart [data-series], .dchart [data-range]")) return void chartClick(e);
  if (e.target.closest(".droute [data-route], .droute [data-maps], .droute [data-stops]")) return void routeClick(e);
  const menuButton = e.target.closest('[data-act="rerunMenu"]');
  if (menuButton) return rerunMenu(menuButton);
  if (!e.target.closest(".dmenu")) closeRerunMenu();
  const copyReply = e.target.closest('[data-act="copyReply"]');
  if (copyReply) {
    copy(copyReply.closest(".turn").querySelector(".reply")?.innerText.trim() ?? "");
    copyReply.innerHTML = ICON.check + "Copied";
    clearTimeout(copyReply.undo);
    copyReply.undo = setTimeout(() => { copyReply.innerHTML = COPY_ICON + "Copy"; }, 1200);
    return;
  }
  const c = e.target.closest("[data-copy]");
  if (c) {
    e.preventDefault();
    copy(c.dataset.copy);
    c.innerHTML = ICON.check;
    clearTimeout(c.undo);
    c.undo = setTimeout(() => { c.innerHTML = COPY_ICON; }, 1200);
    return;
  }
  const img = e.target.closest(".reply img");
  if (img && !img.closest("a, .dprods, .drecs")) {
    return openViewer([{ url: img.currentSrc || img.src, caption: img.alt }], 0);
  }
  const a = e.target.closest("a[href]");
  if (!a) return;
  e.preventDefault();
  invoke("open_url", { url: a.href }).catch(() => {});
});

// Product pictures show over their placeholder once loaded; a broken one
// leaves the placeholder.
bar.addEventListener("load", e => { if (e.target.matches?.(".dprod-img img, .drec img")) e.target.classList.add("on"); }, true);
bar.addEventListener("error", e => { if (e.target.matches?.(".dprod-img img, .drec img")) e.target.remove(); }, true);

// An approval answered in another webview: its card here goes and the chat
// follows the run the answer resumed. The bar has usually settled it already
// from live mode's "answered" event, which comes first.
listen("approval-answered", e => {
  const { session, query_id } = e.payload ?? {};
  const en = live.get(session), pending = en?.approval;
  if (!pending || pending.queryId !== query_id) return;
  en.approval = null;
  pending.card.replaceWith(h('<div class="dmuted">Answered</div>'));
  pending.turn.done = false;
  if (!en.run) live.delete(session);
  if (session !== S.session || S.view !== "chat") return;
  refreshHints();
  refreshSession(session).then(() => watchRun(session));
});

