// The phone layout (spec: tauri-mobile): a bottom tab bar, Back in the top bar in place of
// the sidebar, and long-press for the menus the desktop opens on right-click. The CSS is
// mobile.css, scoped to `body.mobile`; this file sets the class and draws the bars.
import { isMobile } from "../ui/platform.ts";
import { createElement } from "react";
import { MobileTabs, MobileBack, HideKeyboard } from "./mobile-view.tsx";
import { removeWindowPart, showWindowParts } from "./window-root.ts";
import { allTerms } from "../terminals/view.ts";
import { openTile } from "../project/split.ts";
import { pa } from "../project/sections.ts";
import { HOME_TAB_EVENT, homeTab, jump } from "./home.ts";
import { currentLocation, go, goHistory } from "./views.ts";
import { openProjects, paneOn } from "./sidebar.ts";
import { activeTab, backTo, fullScreen, type MobileAt, type MobileTab } from "./mobile-state.ts";
import { $, state } from "./state.ts";
import { atBottom } from "../agents/logscroll.ts";

const OPEN: Record<MobileTab, () => void> = {
  home: () => jump(),
  projects: () => void openProjects(),
  agents: () => void go("pane", "agents"),
  terminals: () => void go("terminals"),
  machines: () => jump("sec-machines"),
};

function at(): MobileAt {
  return {
    page: currentLocation()?.view ?? null,
    paneView: currentLocation()?.view === "pane" ? paneOn().view : null,
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
      return void (currentLocation()?.view === "pane" ? go("pane", "agents") : go("home"));
  }
}

let mounted: { tabs: HTMLElement; top: HTMLElement } | null = null;

function build() {
  const tabs = $("#mb-tabs"), top = $(".cc-top");
  mounted = { tabs, top };
  drawMobile(at());
  // Keep Back before the heading without changing the portal's parent.
  const back = top.querySelector(".mb-back");
  if (back) top.prepend(back);
}

function drawMobile(now: MobileAt) {
  if (!mounted) return;
  showWindowParts([
    { part: "mobile-tabs", host: mounted.tabs, content: createElement(MobileTabs, { active: activeTab(now), needsReply: !!state.home?.needs_you.length, open: tab => OPEN[tab]() }) },
    { part: "mobile-back", host: mounted.top, content: createElement(MobileBack, { hidden: backTo(now) === null, back }) },
  ]);
}

/// Updates the phone controls without replacing their buttons or scroll hosts.
export function syncMobile() {
  if (!document.body.classList.contains("mobile") || !mounted) return;
  const now = at();
  drawMobile(now);
  document.body.classList.toggle("mb-full", fullScreen(now));
  document.body.classList.toggle("mb-home", now.page === "home");
}

function apply() {
  const on = isMobile();
  if (on === document.body.classList.contains("mobile")) return;
  document.body.classList.toggle("mobile", on);
  watchPage(on);
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

const TEXT_FIELD = "textarea, input:not([type]), input[type=text], input[type=search], input[type=url], input[type=email], [contenteditable=true]";

/// The soft keyboard takes the tab bar's place instead of squeezing the page, and the window
/// shrinks to what the keyboard leaves visible, so the composer sits right on top of it.
/// While it is up, the composer being typed in shows a button that puts it away, and dragging
/// the page down puts it away too.
function keyboard() {
  const vv = window.visualViewport;
  if (!vv) return;
  const touch = window.matchMedia("(pointer: coarse)");
  const typing = () => document.activeElement instanceof Element && document.activeElement.matches(TEXT_FIELD);
  const check = () => {
    const up = window.innerHeight - vv.height > 150 || (touch.matches && typing());
    const top = `${vv.offsetTop}px`, height = `${vv.height}px`, root = document.documentElement.style;
    if (up === document.body.classList.contains("mb-kbd") && root.getPropertyValue("--mb-vv-top") === top && root.getPropertyValue("--mb-vv-height") === height) return;

    // A log read to its end stays at its end when the window shrinks or grows.
    const ends = [...document.querySelectorAll<HTMLElement>(".dbody")].filter(log => log.clientHeight && atBottom(log));
    document.body.classList.toggle("mb-kbd", up);
    root.setProperty("--mb-vv-top", top);
    root.setProperty("--mb-vv-height", height);
    for (const log of ends) log.scrollTop = log.scrollHeight;

    if (up) unscroll();
  };
  // iOS scrolls boxes that clip their overflow to bring a focused field into view, which
  // moves the composer off its place; the window already ends at the keyboard.
  const unscroll = () => {
    const field = document.activeElement instanceof HTMLElement && document.activeElement.matches(TEXT_FIELD) ? document.activeElement : null;
    for (let box = field?.parentElement ?? null; box && box !== document.body; box = box.parentElement) {
      if (box.scrollTop && getComputedStyle(box).overflowY === "hidden") box.scrollTop = 0;
    }
  };
  // The keyboard slides in over a few frames, and WebKit does not always report where it
  // stopped, so the window follows it until it settles. Focus moving between fields with the
  // keyboard up leaves the window as it was, but iOS still scrolls toward the new field.
  let settle = 0;
  const follow = () => {
    const until = performance.now() + 800;
    cancelAnimationFrame(settle);
    const step = () => {
      check();
      if (document.body.classList.contains("mb-kbd")) unscroll();
      if (performance.now() < until) settle = requestAnimationFrame(step);
    };
    step();
  };
  vv.addEventListener("resize", check);
  vv.addEventListener("scroll", check);
  document.addEventListener("focusin", follow);
  document.addEventListener("focusout", follow);
  const dismiss = () => (document.activeElement as HTMLElement | null)?.blur();

  document.addEventListener("focusin", e => {
    const field = e.target instanceof Element ? e.target.closest<HTMLElement>(".dinput") : null;
    if (!field || keyboardHost === field || !document.body.classList.contains("mobile")) return;
    keyboardHost = field;
    showWindowParts([{ part: "mobile-keyboard", host: field, content: createElement(HideKeyboard, { dismiss }) }]);
  });

  // A downward drag outside the field, as when scrolling back through the log.
  let startY: number | null = null;
  document.addEventListener("touchstart", e => {
    const inField = e.target instanceof Element && e.target.closest(".dinput");
    startY = inField || !typing() ? null : e.touches[0].clientY;
  }, { passive: true });
  document.addEventListener("touchmove", e => {
    if (startY !== null && e.touches[0].clientY - startY > 40) { startY = null; dismiss(); }
  }, { passive: true });
}

/// Tooltips that name a shortcut lose it; the key handlers stay for a connected keyboard.
const SHORTCUT_TIP = /\s*\((?:[⌘⌃⌥⇧]|Ctrl|esc\b|⏎)[^)]*\)/;

function tips(root: ParentNode) {
  for (const e of root.querySelectorAll<HTMLElement>("[title*='(']")) {
    const t = e.title.replace(SHORTCUT_TIP, "");
    if (t !== e.title) e.title = t;
  }
}

/// The composer showing the button that puts the keyboard away.
let keyboardHost: Element | null = null;
let terminalsQueued = false;

/// One observer, connected only in the phone layout, drops the keyboard button with its
/// composer, strips shortcuts from new tooltips and follows the Terminals page, which opens
/// and closes a terminal without a page change.
const page = new MutationObserver(list => {
  if (keyboardHost && !keyboardHost.isConnected) dropKeyboard();

  const main = $("#main");
  let terminals = false;
  for (const m of list) {
    terminals ||= main.contains(m.target);
    for (const n of m.addedNodes) if (n instanceof Element) tips(n);
  }

  if (!terminals || terminalsQueued || currentLocation()?.view !== "terminals") return;
  terminalsQueued = true;
  requestAnimationFrame(() => { terminalsQueued = false; syncMobile(); });
});

function watchPage(on: boolean) {
  if (!on) { page.disconnect(); dropKeyboard(); return; }

  page.observe(document.body, { childList: true, subtree: true });
}

function dropKeyboard() {
  if (!keyboardHost) return;
  keyboardHost = null;
  removeWindowPart("mobile-keyboard");
}

export function initMobile() {
  build();
  apply();
  window.addEventListener("resize", apply);
  window.matchMedia("(pointer: coarse)").addEventListener("change", apply);
  document.addEventListener(HOME_TAB_EVENT, syncMobile);
  longPress();
  keyboard();
  tips(document);
}
