// The phone layout (spec: tauri-mobile): a bottom tab bar, Back in the top bar in place of
// the sidebar, and long-press for the menus the desktop opens on right-click. The CSS is
// mobile.css, scoped to `body.mobile`; this file sets the class and draws the bars.
import { isMobile } from "../ui/platform.ts";
import { el as h } from "../ui/dom.ts";
import { allTerms } from "../terminals/view.ts";
import { openTile } from "../project/split.ts";
import { pa } from "../project/sections.ts";
import { homeTab, jump } from "./home.ts";
import { go, goHistory, shown } from "./views.ts";
import { openProjects, paneOn } from "./sidebar.ts";
import { activeTab, backTo, fullScreen, TABS, type MobileAt, type MobileTab } from "./mobile-state.ts";
import { $, state } from "./state.ts";

/// Tab icons, as in the phone mocks.
const PATHS: Record<MobileTab, string> = {
  home: "M3 11l9-7 9 7v9h-6v-6H9v6H3z",
  projects: "M3 7h7l2 2h9v11H3z",
  agents: "M4 5h16v11H9l-5 4z M8 9h8M8 12.5h5",
  terminals: "M4 5h16v14H4z M7 9l3 3-3 3M12 15h5",
  machines: "M3 4h18v6H3z M3 14h18v6H3z M7 7h.1M7 17h.1",
};
const BACK = "M15 5l-7 7 7 7";

function svg(path: string, size: number) {
  const s = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  for (const [k, v] of Object.entries({ width: size, height: size, viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", "stroke-width": 1.7, "stroke-linecap": "round", "stroke-linejoin": "round", "aria-hidden": "true" })) s.setAttribute(k, String(v));
  const p = document.createElementNS("http://www.w3.org/2000/svg", "path");
  p.setAttribute("d", path);
  s.append(p);
  return s;
}

const OPEN: Record<MobileTab, () => void> = {
  home: () => jump(),
  projects: () => void openProjects(),
  agents: () => void go("pane", "agents"),
  terminals: () => void go("terminals"),
  machines: () => jump("sec-machines"),
};

function at(): MobileAt {
  return {
    page: shown,
    paneView: shown === "pane" ? paneOn().view : null,
    homeTab,
    picked: state.picked,
    tile: !!pa.tile,
    terminal: !!document.querySelector(".tl > .tm-panel, .tl > .tm-form"),
  };
}

function back() {
  switch (backTo(at())) {
    case "overview": return jump();
    case "tile": return openTile(null);
    case "projects": return void openProjects();
    case "terminals": return allTerms.unselect();
    case "history":
      if (goHistory(false)) return;
      return void (shown === "pane" ? go("pane", "agents") : go("home"));
  }
}

let backBtn: HTMLButtonElement | null = null;
let tabBtns: Map<MobileTab, HTMLButtonElement> | null = null;

function build() {
  const nav = $("#mb-tabs");
  tabBtns = new Map();
  for (const t of TABS) {
    const b = h("button", "", h("span", "ic", svg(PATHS[t.id], 21)), t.label);
    b.type = "button";
    b.addEventListener("click", OPEN[t.id]);
    nav.append(b);
    tabBtns.set(t.id, b);
  }
  backBtn = h("button", "mb-back");
  backBtn.type = "button";
  backBtn.setAttribute("aria-label", "Back");
  backBtn.append(svg(BACK, 22));
  backBtn.addEventListener("click", back);
  $(".cc-top").prepend(backBtn);
}

/// Lights the tab, shows Back and hides the tab bar where the page fills the screen.
/// Called whenever the window draws its top bar.
export function syncMobile() {
  if (!document.body.classList.contains("mobile") || !tabBtns || !backBtn) return;
  const now = at(), tab = activeTab(now);
  for (const [id, b] of tabBtns) {
    if (id === tab) b.setAttribute("aria-current", "true");
    else b.removeAttribute("aria-current");
  }
  // A dot on Home while something needs an answer.
  tabBtns.get("home")!.classList.toggle("dot", !!state.home?.needs_you.length && tab !== "home");
  backBtn.hidden = backTo(now) === null;
  document.body.classList.toggle("mb-full", fullScreen(now));
}

function apply() {
  const on = isMobile();
  if (on === document.body.classList.contains("mobile")) return;
  document.body.classList.toggle("mobile", on);
  syncMobile();
}

/// A long press opens the menu a right-click does: the touch webviews send no `contextmenu`
/// of their own (iOS) or send it late (Android).
function longPress() {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let swallow = 0;
  let from = { x: 0, y: 0 };
  document.addEventListener("touchstart", e => {
    clearTimeout(timer);
    const t = e.touches[0], target = e.target;
    if (e.touches.length !== 1 || !(target instanceof Element) || !document.body.classList.contains("mobile")) return;
    if (target.closest("input, textarea, .tm-xterm, .cc-menu")) return;
    from = { x: t.clientX, y: t.clientY };
    timer = setTimeout(() => {
      swallow = Date.now() + 700;
      target.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: from.x, clientY: from.y }));
    }, 500);
  }, { passive: true });
  document.addEventListener("touchmove", e => {
    const t = e.touches[0];
    if (Math.hypot(t.clientX - from.x, t.clientY - from.y) > 10) clearTimeout(timer);
  }, { passive: true });
  for (const end of ["touchend", "touchcancel"]) document.addEventListener(end, () => clearTimeout(timer), { passive: true });
  // The release after a long press is not a tap.
  document.addEventListener("click", e => {
    if (Date.now() < swallow) { e.preventDefault(); e.stopPropagation(); swallow = 0; }
  }, true);
}

const HIDE_KEYBOARD = "M4 7h16v8H4z M7 10h.1M10 10h.1M13 10h.1M16 10h.1M8 13h8 M9 18l3 3 3-3";

/// The soft keyboard takes the tab bar's place instead of squeezing the page. While it is up,
/// the composer being typed in shows a button that puts it away.
function keyboard() {
  const vv = window.visualViewport;
  if (!vv) return;
  const check = () => document.body.classList.toggle("mb-kbd", window.innerHeight - vv.height > 150);
  vv.addEventListener("resize", check);
  const hide = h("button", "mb-kbdhide", svg(HIDE_KEYBOARD, 20));
  hide.type = "button";
  hide.setAttribute("aria-label", "Hide the keyboard");
  // Pressing it must not take the focus first, or the field loses it before the tap lands.
  hide.addEventListener("pointerdown", e => e.preventDefault());
  hide.addEventListener("click", () => (document.activeElement as HTMLElement | null)?.blur());
  document.addEventListener("focusin", e => {
    const field = e.target instanceof Element ? e.target.closest(".dinput") : null;
    if (field && hide.parentElement !== field) field.append(hide);
  });
}

/// Tooltips that name a shortcut lose it; the key handlers stay for a connected keyboard.
const SHORTCUT_TIP = /\s*\((?:[⌘⌃⌥⇧]|Ctrl|esc\b|⏎)[^)]*\)/;

function tips(root: ParentNode) {
  for (const e of root.querySelectorAll<HTMLElement>("[title*='(']")) {
    const t = e.title.replace(SHORTCUT_TIP, "");
    if (t !== e.title) e.title = t;
  }
}

function plainTips() {
  tips(document);
  new MutationObserver(list => {
    if (!document.body.classList.contains("mobile")) return;
    for (const m of list) for (const n of m.addedNodes) if (n instanceof Element) tips(n);
  }).observe(document.body, { childList: true, subtree: true });
}

export function initMobile() {
  build();
  apply();
  window.addEventListener("resize", apply);
  window.matchMedia("(pointer: coarse)").addEventListener("change", apply);
  longPress();
  keyboard();
  plainTips();
  // The Terminals page opens and closes a terminal without a page change.
  let queued = false;
  new MutationObserver(() => {
    if (queued || shown !== "terminals") return;
    queued = true;
    requestAnimationFrame(() => { queued = false; syncMobile(); });
  }).observe($("#main"), { childList: true, subtree: true });
}
