// The bell at the right end of every command center header, beside the usage
// pill, and the notifications pane it opens: what the assistant told the user
// on its own (a mail digest, an arrival, a review request). These are
// notifications, not chats, until the user replies in one.
import { pane } from "../bar/pane.ts";
import { el as h } from "../ui/dom.ts";
import { ago } from "../ui/format.ts";
import { dbtn as button, svgEl } from "../ui/parts.ts";
import { IC } from "../ui/icons.ts";
import { invoke, listen } from "../ui/tauri.ts";
import { type Alert, type Alerts, badge, bellLabel, find, ids, markRead, subLine, unreadCount, without } from "./notices-state.ts";
import { go } from "./views.ts";

const BELL = '<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M6 16V11a6 6 0 0 1 12 0v5l2 2H4zM10 21h4"/></svg>';

/// Opens the pane, on alert `session` when one is named. Set by `mountNotices`.
let show: (session: string | null) => void = () => {};

/// A notification's click: the pane with that alert opened.
export function showNotices(session: string | null) {
  show(session);
}

/// Draws the bell into `host` and keeps it current. With no server, or one
/// from before alerts, its list is empty.
export function mountNotices(host: HTMLElement) {
  let alerts: Alerts = null;
  let open = false;
  let opened: string | null = null;
  /// An alert a click asked for before the list had come.
  let wanted: string | null = null;
  host.classList.add("nt");

  const set = (next: Alerts) => {
    // An opened alert that did not change keeps its view, and the reply typed in it.
    const same = open && opened && JSON.stringify(find(alerts, opened)) === JSON.stringify(find(next, opened));
    alerts = next;
    if (same) return drawBell();
    if (wanted && find(alerts, wanted)) opened = wanted;
    wanted = null;
    // An opened alert that is gone, dismissed or a chat now, shows the list.
    if (opened && !find(alerts, opened)) opened = null;
    if (opened) read([opened]);
    draw();
  };

  const toggle = (to: boolean) => {
    open = to;
    if (!to) opened = null;
    draw();
  };

  const makeBell = () => {
    const count = badge(alerts);
    const bell = h("button", "nt-bell", svgEl(BELL)!, count ? h("span", "nt-n", count) : null);
    bell.type = "button";
    bell.title = "Notifications";
    bell.setAttribute("aria-label", bellLabel(alerts));
    bell.setAttribute("aria-expanded", String(open));
    bell.addEventListener("click", () => toggle(!open));
    return bell;
  };

  /// Redraws the bell alone, leaving the pane as it is.
  const drawBell = () => {
    const first = host.firstElementChild;
    if (first) first.replaceWith(makeBell());
    else draw();
  };

  const draw = () => {
    const bell = makeBell();
    const shown = opened && find(alerts, opened);
    host.replaceChildren(bell, ...(open ? [shown ? openedView(shown) : listView(alerts ?? [])] : []));
  };

  const read = (list: string[]) => {
    const fresh = list.filter(id => find(alerts, id)?.read === false);
    if (!fresh.length) return;
    alerts = markRead(alerts, fresh);
    invoke("alerts_read", { ids: fresh }).catch(e => console.warn("reading alerts:", e));
  };

  const dismiss = (list: string[]) => {
    if (!list.length) return;
    set(without(alerts, list));
    invoke("alerts_dismiss", { ids: list }).catch(e => {
      console.warn("dismissing alerts:", e);
      void load();
    });
  };

  const openAlert = (id: string) => {
    opened = id;
    read([id]);
    draw();
  };

  /// A reply turns the alert into a chat: it leaves the pane and the chat opens with the reply sent.
  const reply = (a: Alert, text: string) => {
    const message = text.trim();
    if (!message) return;
    alerts = without(alerts, [a.session_id]);
    toggle(false);
    go("pane", { chat: { session: a.session_id } });
    pane.ask(a.session_id, message).catch(e => console.warn("replying to an alert:", e));
  };

  const listView = (list: Alert[]) => {
    const unread = unreadCount(list);
    const head = h("div", "nt-h", h("b", "", "Notifications"), unread ? h("span", "", `${unread} unread`) : null,
      list.length ? button("Clear", "small quiet", () => dismiss(ids(list))) : null);
    const body = list.length
      ? list.map(row)
      : [h("div", "nt-empty", "No notifications")];
    return h("div", "nt-pop", head, h("div", "nt-list", ...body));
  };

  const row = (a: Alert) => {
    const close = button(svgEl(IC.x)!, "quiet", e => { e.stopPropagation(); dismiss([a.session_id]); });
    close.setAttribute("aria-label", "Dismiss");
    close.title = "Dismiss";
    const text = h("span", "nt-t", h("b", "", a.title), h("span", "", subLine(a, ago(a.at))));
    const r = h("div", `nt-row${a.read ? "" : " unread"}`, h("span", "nt-dot"), text, close);
    r.tabIndex = 0;
    r.setAttribute("role", "button");
    r.addEventListener("click", () => openAlert(a.session_id));
    r.addEventListener("keydown", e => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); openAlert(a.session_id); } });
    return r;
  };

  const openedView = (a: Alert) => {
    const back = button("Notifications", "small quiet", () => { opened = null; draw(); });
    const head = h("div", "nt-h", back, h("span", "", subLine(a, ago(a.at))), button("Dismiss", "small", () => dismiss([a.session_id])));
    const input = h("input", "nt-ask");
    input.type = "text";
    input.placeholder = "Reply to turn this into a chat";
    input.setAttribute("aria-label", "Reply");
    input.autocomplete = "off";
    input.spellcheck = false;
    input.addEventListener("keydown", e => { if (e.key === "Enter" && !e.isComposing) { e.preventDefault(); reply(a, input.value); } });
    const send = button("Send", "pri small", () => reply(a, input.value));
    // The text is the assistant's own, shown as text and never as markup.
    const body = h("div", "nt-list nt-note", h("b", "", a.title), h("p", "", a.text));
    const pop = h("div", "nt-pop", head, body, h("div", "nt-rep", input, send));
    queueMicrotask(() => input.focus());
    return pop;
  };

  show = session => {
    open = true;
    opened = session && find(alerts, session) ? session : null;
    wanted = opened ? null : session;
    if (opened) read([opened]);
    draw();
  };

  const load = async () => {
    try {
      set(await invoke<Alerts>("alerts_list"));
    } catch (e) {
      console.warn("reading alerts:", e);
    }
  };

  document.addEventListener("click", e => { if (open && !e.composedPath().includes(host)) toggle(false); });
  document.addEventListener("keydown", e => {
    if (!open || e.key !== "Escape") return;
    // Esc goes back to the list first, then closes the pane.
    if (opened) { opened = null; draw(); } else toggle(false);
  });
  listen<Alerts>("alerts", e => set(e.payload));
  for (const name of ["link-state", "reconnected"]) listen(name, () => void load());
  draw();
  void load();
}
