import { el } from "../ui/dom.ts";
import { nextMatch, searchPattern, textMatches } from "./find-state.ts";

export interface SearchSource { key: string; root: HTMLElement }
export interface ChatSearch {
  body: HTMLElement & { stuck?: boolean };
  sources: () => SearchSource[];
  reveal: (key: string) => HTMLElement | null;
  prepare?: () => Promise<void>;
  watch?: (changed: () => void) => () => void;
}
interface Match { key: string; at: number }

let active: { close: (focus?: boolean) => void; focus: () => void; move: (step: number) => void } | null = null;

export function stepChatFind(step: number) {
  if (!active) return false;
  active.move(step);
  return true;
}

export function closeChatFind() {
  if (!active) return false;
  active.close();
  return true;
}

/** A find bar scoped to one conversation, including turns outside the DOM. */
export function openChatFind(chat: ChatSearch) {
  if (active && document.querySelector(".chat-find")) { active.focus(); return; }
  active?.close(false);
  const { body } = chat;
  const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  const bar = el("div", "chat-find");
  bar.setAttribute("role", "search");
  bar.setAttribute("aria-label", "Search conversation");
  const input = el("input");
  input.placeholder = "Find in chat";
  input.setAttribute("aria-label", "Find in chat");
  input.setAttribute("aria-describedby", "chat-find-status");
  input.autocomplete = "off";
  input.spellcheck = false;
  const status = el("output");
  status.id = "chat-find-status";
  status.setAttribute("aria-live", "polite");
  const button = (label: string, path: string, run: () => void) => {
    const b = el("button");
    b.type = "button";
    b.setAttribute("aria-label", label);
    b.title = label;
    b.innerHTML = `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="${path}"/></svg>`;
    b.onclick = run;
    return b;
  };
  const prev = button("Previous match (Shift+Enter)", "m18 15-6-6-6 6", () => move(-1));
  const next = button("Next match (Enter)", "m6 9 6 6 6-6", () => move(1));
  const exit = button("Close search (Esc)", "m6 6 12 12M6 18 18 6", () => close());
  const regex = el("button", "chat-find-regex", ".*");
  regex.type = "button";
  regex.title = "Use regular expression";
  regex.setAttribute("aria-label", "Use regular expression");
  regex.setAttribute("aria-pressed", "false");
  regex.onclick = () => {
    regex.setAttribute("aria-pressed", String(regex.getAttribute("aria-pressed") !== "true"));
    clearTimeout(timer); timer = 0;
    refresh(true);
    input.focus({ preventScroll: true });
  };
  bar.append(input, regex, status, prev, next, exit);
  const layer = el("div", "chat-find-layer");
  layer.append(bar);
  body.before(layer);
  let matches: Match[] = [], current = -1, timer = 0, updateTimer = 0, loading = !!chat.prepare, closed = false;
  let pattern: RegExp | null = null, error = "";
  let marks: HTMLElement[] = [];
  const observe = () => observer.observe(body, { childList: true, subtree: true, characterData: true });
  const changed = () => {
    updateTimer ||= window.setTimeout(() => { updateTimer = 0; refresh(false); }, 120);
  };
  const observer = new MutationObserver(changed);
  const unwatch = chat.watch?.(changed);
  const navigation = new MutationObserver(() => { if (!bar.isConnected || !body.isConnected) close(false); });
  navigation.observe(document.body, { childList: true, subtree: true });

  function clearMarks() {
    for (const mark of marks) {
      const parent = mark.parentNode;
      if (parent) { mark.replaceWith(...mark.childNodes); parent.normalize(); }
    }
    marks = [];
  }
  function highlight() {
    observer.disconnect();
    clearMarks();
    const chosen = matches[current];
    const log = body.querySelector(".dcoderlog");
    const roots = log ? [...log.querySelectorAll<HTMLElement>(":scope > .agturn")].map(root => ({ key: root.dataset.findKey!, root })) : [{ key: "chat", root: body }];
    let selected: HTMLElement | undefined;
    for (const { key, root } of roots) {
      const ranges = matchingRanges(root, pattern);
      // Wrap from the end so earlier offsets stay valid, including inline code.
      for (let at = ranges.length - 1; at >= 0; at--) {
        const on = chosen?.key === key && chosen.at === at;
        for (const range of ranges[at].slice().reverse()) {
          const mark = el("mark", `chat-match${on ? " current" : ""}`);
          range.surroundContents(mark);
          marks.push(mark);
          if (on) selected = mark;
        }
      }
    }
    observe();
    return selected;
  }
  function count() {
    status.textContent = error ? "Invalid regex" : loading ? "Loading history..." : !input.value ? "" : matches.length ? `${current + 1} of ${matches.length}` : "No matches";
    prev.disabled = next.disabled = !matches.length;
  }
  function refresh(jump: boolean) {
    if (closed) return;
    const chosen = matches[current];
    try {
      pattern = searchPattern(input.value, regex.getAttribute("aria-pressed") === "true");
      error = "";
    } catch (e) {
      pattern = null;
      error = e instanceof Error ? e.message : String(e);
    }
    bar.classList.toggle("invalid", !!error);
    if (error) { input.setAttribute("aria-invalid", "true"); status.title = error; }
    else { input.removeAttribute("aria-invalid"); status.removeAttribute("title"); }
    matches = pattern ? chat.sources().flatMap(({ key, root }) => matchingRanges(root, pattern).map((_, at) => ({ key, at }))) : [];
    const kept = chosen ? matches.findIndex(m => m.key === chosen.key && m.at === chosen.at) : -1;
    current = matches.length ? jump ? 0 : kept >= 0 ? kept : Math.min(Math.max(0, current), matches.length - 1) : -1;
    count();
    if (jump) show(); else highlight();
  }
  function show() {
    const match = matches[current];
    if (match) {
      body.stuck = false;
      chat.reveal(match.key);
    }
    const selected = highlight();
    if (selected) {
      // Open command output and thoughts can scroll inside the conversation.
      for (let parent = selected.parentElement; parent; parent = parent.parentElement) {
        const rect = selected.getBoundingClientRect(), area = parent.getBoundingClientRect();
        if (parent === body || /auto|scroll/.test(getComputedStyle(parent).overflowY)) {
          parent.scrollTop += rect.top - area.top - parent.clientHeight / 2 + rect.height / 2;
        }
        if (/auto|scroll/.test(getComputedStyle(parent).overflowX)) {
          if (rect.left < area.left) parent.scrollLeft += rect.left - area.left;
          else if (rect.right > area.right) parent.scrollLeft += rect.right - area.right;
        }
        if (parent === body) break;
      }
    }
  }
  function move(step: number) {
    if (timer) { clearTimeout(timer); timer = 0; refresh(false); }
    current = nextMatch(current, matches.length, step);
    count(); show();
  }
  function close(focus = true) {
    if (closed) return;
    closed = true;
    clearTimeout(timer); clearTimeout(updateTimer);
    observer.disconnect(); navigation.disconnect();
    unwatch?.();
    clearMarks(); layer.remove();
    if (active?.close === close) active = null;
    if (focus && previous?.isConnected) previous.focus({ preventScroll: true });
  }
  input.addEventListener("input", () => { clearTimeout(timer); timer = window.setTimeout(() => { timer = 0; refresh(true); }, 100); });
  bar.addEventListener("keydown", e => {
    if (e.isComposing) return;
    if (e.key === "Enter" && e.target === input) { e.preventDefault(); e.stopPropagation(); move(e.shiftKey ? -1 : 1); }
  });
  active = { close, move, focus: () => { input.focus(); input.select(); } };
  active.focus();
  observe(); count();
  if (chat.prepare) void chat.prepare().then(() => { if (!closed) { loading = false; refresh(true); } }, () => { if (!closed) { loading = false; refresh(false); status.textContent = "Could not load older messages"; } });
}

const BLOCK = "p,pre,li,h1,h2,h3,h4,h5,h6,td,th,blockquote,summary,.dcoderen,.duser,.danswer,.dask,.dmsg";

/** Match across inline markup, but never across separate messages or paragraphs. */
export function matchingRanges(root: HTMLElement, pattern: RegExp | null): Range[][] {
  if (!pattern) return [];
  const groups: { block: Element | null; nodes: Text[]; text: string }[] = [];
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  while (walker.nextNode()) {
    const node = walker.currentNode as Text, parent = node.parentElement;
    if (!node.data || !parent || parent.closest('script,style,button,textarea,[hidden],[aria-hidden="true"],.dthink:not(.open) .bd,.dtrun:not(.open) .bd,.dcoderen.mf:not(.open) .mf-grp')) continue;
    const details = parent.closest("details:not([open])");
    if (details && !details.querySelector("summary")?.contains(node)) continue;
    const block = parent.closest(BLOCK) ?? parent.closest("div") ?? root;
    let group = groups.at(-1);
    if (!group || group.block !== block) { group = { block, nodes: [], text: "" }; groups.push(group); }
    group.nodes.push(node); group.text += node.data;
  }
  return groups.flatMap(group => {
    let first = 0, offset = 0;
    return textMatches(group.text, pattern).map(match => {
      while (first < group.nodes.length && offset + group.nodes[first].length <= match.start) offset += group.nodes[first++].length;
      const ranges: Range[] = [];
      let start = offset;
      for (let i = first; i < group.nodes.length && start < match.end; i++) {
        const node = group.nodes[i], range = document.createRange();
        range.setStart(node, Math.max(0, match.start - start));
        range.setEnd(node, Math.min(node.length, match.end - start));
        ranges.push(range);
        start += node.length;
      }
      return ranges;
    });
  });
}
