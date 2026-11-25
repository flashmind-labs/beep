// The phone layout (spec: tauri-mobile): a bottom tab bar, Back in the top bar in place of
// the sidebar, and long-press for the menus the desktop opens on right-click. The CSS is
// mobile.css, scoped to `body.mobile`; this file sets the class and draws the bars.
import { isMobile, onPhone } from "../ui/platform.ts";
import { invoke } from "../ui/tauri.ts";
import { MobileTabs, MobileBack, MobileSettings, HideKeyboard } from "./mobile-view.tsx";
import { placeWindowParts, removeWindowPart, showWindowParts } from "./window-root.tsx";
import { windowHost } from "./window-shell.tsx";
import { allTerms } from "./allterms.ts";
import { openTile } from "../project/split.tsx";
import { pa } from "../project/barstate.ts";
import { HOME_TAB_EVENT, homeTab, jump } from "./home.tsx";
import { currentLocation, go, goHistory, asBack } from "./nav.ts";
import { drawRail, paneOn } from "./sidebar.tsx";
import { openProjects } from "../project/addflow.ts";
import { openSettings } from "../settings/sheet.ts";
import { activeTab, backTo, fullScreen, type MobileAt, type MobileTab } from "./mobile-state.ts";
import { state } from "./state.ts";
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

/// The page showing has a Back.
export function canGoBack() {
  return backTo(at()) !== null;
}

function back() {
  asBack(up);
}

function up() {
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

let mounted = false;

function drawMobile(now: MobileAt) {
  if (!mounted) return;
  placeWindowParts({
    "mobile-tabs": <MobileTabs active={activeTab(now)} needsReply={!!state.home?.needs_you.length} open={tab => OPEN[tab]()} />,
    "mobile-back": <MobileBack hidden={backTo(now) === null} back={back} />,
    "mobile-settings": <MobileSettings hidden={backTo(now) !== null} open={() => openSettings()} />,
  });
}

/// Updates the phone controls without replacing their buttons or scroll hosts.
export function syncMobile() {
  if (!document.body.classList.contains("mobile") || !mounted) return;
  const now = at();
  drawMobile(now);
  document.body.classList.toggle("mb-full", fullScreen(now));
  document.body.classList.toggle("mb-home", now.page === "home");
  // The composer that showed the keyboard button may have gone with its page.
  if (keyboardHost && !keyboardHost.isConnected) dropKeyboard();
}

function apply() {
  const on = isMobile();
  if (on === document.body.classList.contains("mobile")) return;
  document.body.classList.toggle("mobile", on);
  watchPage(on);
  syncMobile();
  drawRail();
}

/// A long press opens the menu a right-click does, in either layout on a phone: the touch webviews send no `contextmenu`
/// of their own (iOS) or send it late (Android).
function longPress() {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let swallow = 0;
  let from = { x: 0, y: 0 };
  document.addEventListener("touchstart", e => {
    clearTimeout(timer);
    const t = e.touches[0], target = e.target;
    if (e.touches.length !== 1 || !(target instanceof Element) || !(onPhone() || document.body.classList.contains("mobile"))) return;
    if (target.closest("input, textarea, .tm-term, .cc-menu")) return;
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

/// How near the left edge a swipe back starts, and how far right it must go to count.
const EDGE_PX = 24, BACK_PX = 80;

/// A swipe right from the left edge goes back, as the Back button does. The page follows the
/// finger on its own layer; a swipe that turns vertical or stops short springs back.
function swipeBack() {
  let start: { x: number; y: number } | null = null, dragging = false;
  const main = () => document.querySelector<HTMLElement>(".cc-main");
  const reset = (animate: boolean) => {
    const page = main();
    start = null;
    dragging = false;
    if (!page) return;
    page.style.transition = animate ? "transform .18s ease-out" : "";
    page.style.transform = "";
    if (animate) page.addEventListener("transitionend", () => { page.style.transition = ""; page.style.willChange = ""; }, { once: true });
    else page.style.willChange = "";
  };

  document.addEventListener("touchstart", e => {
    const t = e.touches[0];
    start = e.touches.length === 1 && t.clientX <= EDGE_PX && document.body.classList.contains("mobile") && backTo(at()) !== null
      ? { x: t.clientX, y: t.clientY } : null;
  }, { passive: true });
  document.addEventListener("touchmove", e => {
    if (!start) return;
    const t = e.touches[0], dx = t.clientX - start.x, dy = t.clientY - start.y;
    if (!dragging && Math.abs(dy) > Math.max(10, dx)) { reset(false); return; }
    if (dx < 10 && !dragging) return;
    const page = main();
    if (!page) return;
    dragging = true;
    page.style.willChange = "transform";
    page.style.transform = `translateX(${Math.max(0, dx)}px)`;
  }, { passive: true });
  document.addEventListener("touchend", e => {
    if (!start) return;
    const went = dragging && (e.changedTouches[0]?.clientX ?? 0) - start.x >= BACK_PX;
    reset(!went);
    if (went) back();
  }, { passive: true });
  document.addEventListener("touchcancel", () => { if (start) reset(true); }, { passive: true });
}

const TEXT_FIELD = "textarea, input:not([type]), input[type=text], input[type=search], input[type=url], input[type=email], [contenteditable=true]";
/// The buttons that send or start what a composer's field holds.
const SEND = "#agsend, .cc-pjc button[type=submit], .cc-comp button[type=submit], .nt-rep .dbtn.pri";

/// The soft keyboard takes the tab bar's place instead of squeezing the page, and the window
/// shrinks to what the keyboard leaves visible, so the composer sits right on top of it.
/// While it is up, the composer being typed in shows a button that puts it away, and dragging
/// the page down puts it away too.
function keyboard() {
  const vv = window.visualViewport;
  if (!vv) return;
  const touch = window.matchMedia("(pointer: coarse)");
  const typing = () => document.activeElement instanceof Element && document.activeElement.matches(TEXT_FIELD);
  const dismiss = () => (document.activeElement as HTMLElement | null)?.blur();
  // iOS shrinks innerHeight with the keyboard but not the layout viewport, so the keyboard shows
  // as the visual viewport falling short of it. Android resizes both and has only `typing()`.
  let covered = false;
  const check = () => {
    const short = document.documentElement.clientHeight - vv.height > 150;
    // iOS can put the keyboard away and leave the field focused, as when another app or the
    // system takes it. The page would stay laid out for the keyboard, offset by where the
    // window had scrolled, so the field lets go and the window scrolls back.
    if (covered && !short && typing()) {
      covered = false;
      dismiss();
      window.scrollTo(0, 0);
    }
    covered = short;
    const up = short || (touch.matches && typing());
    const top = `${vv.offsetTop}px`, height = `${vv.height}px`, root = document.documentElement.style;
    if (up === document.body.classList.contains("mb-kbd") && root.getPropertyValue("--mb-vv-top") === top && root.getPropertyValue("--mb-vv-height") === height) return;

    // A log read to its end stays at its end when the window shrinks or grows.
    const ends = [...document.querySelectorAll<HTMLElement>(".dbody")].filter(log => log.clientHeight && atBottom(log));
    document.body.classList.toggle("mb-kbd", up);
    root.setProperty("--mb-vv-top", top);
    root.setProperty("--mb-vv-height", height);
    for (const log of ends) log.scrollTop = log.scrollHeight;
    // The window moved under any open menu: they place themselves again on a resize.
    window.dispatchEvent(new Event("resize"));

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
  // Only the phone layout has a soft keyboard to follow.
  const follow = () => {
    if (!document.body.classList.contains("mobile")) return;
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

  // A tap on a send button, or any button in the field, keeps the field focused. Losing focus would
  // put the keyboard away and move the button before the click lands, so the first tap only hid the keyboard.
  // WebKit moves focus on mousedown even when pointerdown was prevented.
  document.addEventListener("mousedown", e => {
    if (document.body.classList.contains("mobile") && typing() && e.target instanceof Element && e.target.closest(`${SEND}, .dinput button`)) e.preventDefault();
  }, true);

  document.addEventListener("focusin", e => {
    const field = e.target instanceof Element ? e.target.closest<HTMLElement>(".dinput") : null;
    if (!field || keyboardHost === field || !document.body.classList.contains("mobile")) return;
    keyboardHost = field;
    showWindowParts([{ part: "mobile-keyboard", host: field, content: <HideKeyboard dismiss={dismiss} /> }]);
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

/// Connected only in the phone layout, and only on the page's main area and the overlays: strips shortcuts from
/// new tooltips there and follows the Terminals page, which opens and closes a terminal without
/// a page change. The rest of the window, and the desktop layout, cost it nothing.
const page = new MutationObserver(list => {
  for (const m of list) for (const n of m.addedNodes) if (n instanceof Element) tips(n);

  if (terminalsQueued || currentLocation()?.view !== "terminals") return;
  terminalsQueued = true;
  requestAnimationFrame(() => { terminalsQueued = false; syncMobile(); });
});

/// Sheets, menus and the palette open outside the main area, as children of the body or
/// of the window: each that opens loses its shortcuts, and so does what it draws later.
const overlays = new MutationObserver(list => {
  for (const m of list) for (const n of m.addedNodes) {
    if (!(n instanceof Element)) continue;
    tips(n);
    page.observe(n, { childList: true, subtree: true });
  }
});

function watchPage(on: boolean) {
  if (!on) { page.disconnect(); overlays.disconnect(); dropKeyboard(); return; }

  const main = windowHost("main");
  page.observe(main, { childList: true, subtree: true });
  overlays.observe(document.body, { childList: true });
  if (main.parentElement && main.parentElement !== document.body) overlays.observe(main.parentElement, { childList: true });
}

function dropKeyboard() {
  if (!keyboardHost) return;
  keyboardHost = null;
  removeWindowPart("mobile-keyboard");
}

/// A phone that moves to another network dials its machines again: Android tells native code
/// nothing, and links over the old network can look open while dead. A quality estimate that
/// changes on the same network is no move.
function watchNetwork() {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const moved = () => {
    clearTimeout(timer);
    timer = setTimeout(() => void invoke("mobile_network_changed").catch(e => console.warn("network change:", e)), 1000);
  };
  window.addEventListener("online", moved);

  const connection: unknown = Reflect.get(navigator, "connection");
  if (!(connection instanceof EventTarget)) return;
  let kind: unknown = Reflect.get(connection, "type");
  connection.addEventListener("change", () => {
    const now: unknown = Reflect.get(connection, "type");
    if (now === kind) return;
    kind = now;
    moved();
  });
}

export function initMobile() {
  mounted = true;
  // Styles that differ on a phone in either layout, such as the narrower folded sidebar.
  document.documentElement.toggleAttribute("data-phone", onPhone());
  drawMobile(at());
  apply();
  window.addEventListener("resize", apply);
  window.matchMedia("(pointer: coarse)").addEventListener("change", apply);
  document.addEventListener(HOME_TAB_EVENT, syncMobile);
  longPress();
  swipeBack();
  keyboard();
  tips(document);
  if (onPhone()) {
    watchNetwork();
    void window.__TAURI__.app?.onBackButtonPress(systemBack);
  }
}

/// Android's back button: closes what floats, then goes back a page, and
/// leaves the app from the first page.
function systemBack() {
  const open = document.querySelector<HTMLElement>('[role="dialog"], [role="menu"], [role="listbox"]');
  if (open) {
    (document.activeElement ?? open).dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    return;
  }

  if (document.body.classList.contains("mobile") && canGoBack()) return back();
  if (!goHistory(false)) void invoke("quit");
}
