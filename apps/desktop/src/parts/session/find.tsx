import { createRef } from "react";
import { flushSync } from "react-dom";
import { createDocumentView } from "../../ui/document-root.tsx";
import { glide } from "../../ui/motion.ts";
import { FindView } from "./find-view.tsx";
import { nextMatch, searchPattern, textMatches } from "../../stores/chat/find-state.ts";

interface SearchSource { key: string; root: HTMLElement }
interface ChatSearch {
  body: HTMLElement & { stuck?: boolean };
  sources: () => SearchSource[];
  reveal: (key: string) => HTMLElement | null;
  prepare?: () => Promise<void>;
  watch?: (changed: () => void) => () => void;
}
interface Match { key: string; at: number }

let active: { close: (focus?: boolean) => void; focus: () => void; move: (step: number) => void; shown: () => boolean } | null = null;

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
  if (active?.shown()) { active.focus(); return; }
  active?.close(false);
  const { body } = chat;
  const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  // The bar sits above the conversation it searches, in a host of its own.
  const layer = document.createElement("div");
  layer.className = "chat-find-layer";
  body.before(layer);
  const field = createRef<HTMLInputElement>();
  const root = createDocumentView(layer);
  let useRegex = false, statusText = "";
  const draw = () => flushSync(() => root.render(<FindView
    field={field} error={error} status={statusText} regex={useRegex} disabled={!matches.length}
    input={() => { clearTimeout(timer); timer = window.setTimeout(() => { timer = 0; refresh(true); }, 100); }}
    enter={reverse => move(reverse ? -1 : 1)}
    toggleRegex={() => { useRegex = !useRegex; clearTimeout(timer); timer = 0; refresh(true); input.focus({ preventScroll: true }); }}
    move={move} close={() => close()} />));
  let input: HTMLInputElement;
  let matches: Match[] = [], current = -1, timer = 0, updateTimer = 0, loading = !!chat.prepare, closed = false;
  let pattern: RegExp | null = null, error = "";
  let marks: HTMLElement[] = [];
  const observe = () => observer.observe(body, { childList: true, subtree: true, characterData: true });
  const changed = () => {
    updateTimer ||= window.setTimeout(() => { updateTimer = 0; refresh(false); }, 120);
  };
  const observer = new MutationObserver(changed);
  const unwatch = chat.watch?.(changed);
  const navigation = new MutationObserver(() => { if (!layer.isConnected || !body.isConnected) close(false); });
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
          // Matches are marked inside the rendered messages, as the browser's own find does.
          const mark = document.createElement("mark");
          mark.className = on ? "chat-match current" : "chat-match";
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
    statusText = error ? "Invalid regex" : loading ? "Loading history..." : !input.value ? "" : matches.length ? `${current + 1} of ${matches.length}` : "No matches";
    draw();
  }
  function refresh(jump: boolean) {
    if (closed) return;
    const chosen = matches[current];

    try {
      pattern = searchPattern(input.value, useRegex);
      error = "";
    } catch (e) {
      pattern = null;
      error = e instanceof Error ? e.message : String(e);
    }

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
        const by = rect.top - area.top - parent.clientHeight / 2 + rect.height / 2;
        // The conversation glides to the match and lights its line; an inner scroller jumps.
        if (parent === body) glide(body, body.scrollTop + by, "glide", { landed: selected.parentElement, done: null });
        else if (/auto|scroll/.test(getComputedStyle(parent).overflowY)) parent.scrollTop += by;

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
    clearMarks(); root.unmount(); layer.remove();
    if (active?.close === close) active = null;
    if (focus && previous?.isConnected) previous.focus({ preventScroll: true });
  }
  draw();
  if (!field.current) { root.unmount(); layer.remove(); return; }
  input = field.current;
  active = { close, move, shown: () => layer.isConnected, focus: () => { input.focus(); input.select(); } };
  active.focus();
  observe(); count();
  if (chat.prepare) void chat.prepare().then(() => { if (!closed) { loading = false; refresh(true); } }, () => { if (!closed) { loading = false; refresh(false); statusText = "Could not load older messages"; draw(); } });
}

const BLOCK = "p,pre,li,h1,h2,h3,h4,h5,h6,td,th,blockquote,summary,.dcoderen,.duser,.danswer,.dask,.dmsg";

/** Match across inline markup, but never across separate messages or paragraphs. */
function matchingRanges(root: HTMLElement, pattern: RegExp | null): Range[][] {
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
