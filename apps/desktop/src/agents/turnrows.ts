// The rows of a chat log, drawn the same in a Beep chat and a coding agent's
// session: the prompt, a turn's heading, a thought, a tool call, the answer,
// a failure, a queued message and the Stop button.
import { quotedMarkdown } from "./copy-markdown.ts";
import { clock, duration, heading, thoughtLabel, KIND_ICONS, type ToolKind, type TurnState } from "./agentturns.ts";
import { copy, writeClipboard } from "../ui/parts.ts";
import { endedHtml, liveHtml } from "./thoughtlive.ts";
import { esc } from "../ui/format.ts";
import { COPY_ICON, ICON } from "../ui/icons.ts";
import { onShownChange, paneShown } from "../ui/active.ts";

/// `word`, its letters lit one after another in a loop.
const sweptLabel = (word: string) => `<span class="lb">${esc(word)}</span>`;

/// A running turn's one live line: what it does now (its last thought line),
/// or "Thinking" or "Working" until there is one, its letters sweeping, and
/// how long it has run. Waiting on the user does not sweep.
export function liveLineHtml(t: { state: TurnState; thinking: boolean; start?: number | null; doing?: string }) {
  const waiting = t.state === "waiting";
  const lead = t.doing || (t.thinking ? "Thinking" : "Working");
  const word = waiting ? `<span class="lb">Waiting for you</span>` : sweptLabel(lead).replace('<span class="lb">', `<span class="lb" title="${esc(lead)}">`);
  return `<div class="dthink dlive${waiting ? "" : " live"}">${word}`
    + (t.start != null ? `<span class="since" data-since="${t.start}">${esc(duration(Date.now() - t.start))}</span>` : "") + "</div>";
}

/// A thought, as one line that opens to its text. `live` streams and follows
/// its newest line; `folding` just ended and folds now. `row` is its place in
/// the log, which its button names; else `key` does.
export function thoughtHtml(t: { key: string; text: string; html?: string; ms?: number | null; live: boolean; open: boolean; folding: boolean; row?: number }) {
  const body = t.live ? `<div class="tx">${liveHtml(t.key, t.text)}</div>`
    : t.folding ? `<div class="tx">${endedHtml(t.key, t.text)}</div>`
    : `<div class="tx md">${t.html ?? esc(t.text).replace(/\n/g, "<br>")}</div>`;
  return `<div class="dthink${t.live ? " live" : ""}${t.open ? " open" : ""}"${t.row != null ? ` data-row="${t.row}"` : ""}${t.folding ? " data-folding" : ""}>`
    + `<button data-thought="${esc(t.row ?? t.key)}" aria-expanded="${t.open}"><span class="lb">${thoughtLabel(t.ms)}</span><span class="cv">${ICON.chev}</span></button>`
    + `<div class="bd"><div>${body}</div></div></div>`;
}

/// Opens or folds the thought of button `btn` in place; true when open now.
export function toggleThought(btn: Element) {
  const open = !btn.parentElement?.classList.contains("open");
  btn.parentElement?.classList.toggle("open", open);
  btn.setAttribute("aria-expanded", String(open));
  return open;
}

/// A turn's heading: how long it worked and what it did. With a `key` it is
/// a button that opens or folds the steps.
export function turnHeadHtml(t: { state: TurnState; start?: number | null; ms: number | null; stats: string; key: string | number | null; opened: boolean }) {
  const done = t.state === "done";
  // A running turn's clock is on its live line; its head only folds the steps.
  if (!done && t.key == null) return "";
  const label = done ? `<span>${esc(heading(t.state, t.ms))}</span>` + (t.stats ? `<span class="stats">· ${esc(t.stats)}</span>` : "")
    : `<span>${esc(t.stats || "Steps")}</span>`;
  return `<div class="dcoderturn">`
    + (t.key == null ? `<span class="hd">${label}</span>`
      : `<button class="hd${t.opened ? " open" : ""}" data-turn="${esc(t.key)}" aria-expanded="${t.opened}" title="${t.opened ? "Fold the steps" : "Show the steps"}">${label}${ICON.chev}</button>`)
    + (t.start != null ? `<span class="when">${clock(t.start)}</span>` : "") + "</div>";
}

export const collapseActivityHtml = (key: number) => `<div><button class="dprogress-collapse" data-turn="${key}" data-collapse aria-label="Collapse activity" title="Collapse activity" aria-expanded="true">${ICON.chev}</button></div>`;

let turnTick: ReturnType<typeof setInterval> | undefined;

/// Moves the running turns' clocks on each second, until none shows or
/// the pane hides.
export function tickTurns() {
  if (turnTick || !paneShown() || !document.querySelector("[data-since]")) return;
  turnTick = setInterval(() => {
    const els = document.querySelectorAll<HTMLElement>("[data-since]");
    if (!els.length || !paneShown()) { clearInterval(turnTick); turnTick = undefined; return; }

    for (const el of els) {
      const text = duration(Date.now() - Number(el.dataset.since));
      if (el.textContent !== text) el.textContent = text;
    }
  }, 1000);
}
onShownChange(shown => { if (shown) tickTurns(); }, true);

/// A step's mark: done, failed, running or did not finish.
export function stepMark(status: string, running: boolean) {
  return status === "completed" ? `<span class="dok" title="Done">${ICON.check}</span>`
    : status === "failed" ? `<span class="dbad" title="Failed">${ICON.x}</span>`
    : status && running ? `<span class="dspin" title="Running"></span>`
    : status ? '<span class="ddot off" title="Did not finish"></span>' : "";
}

export { KIND_ICONS } from "./agentturns.ts";

/// A tool call as one row: icon, verb, its target in mono, the right side
/// (an edit's +2 -1 in color, an exit code) and its mark. `cls` and `attrs`
/// go on the row, `after` inside it.
export function toolRowHtml(r: { kind: ToolKind; verb: string; target: string; tip: string; detail: string; bad: boolean; mark: string; since?: number; cls?: string; attrs?: string; after?: string }) {
  const counts = /^\+(\d+) -(\d+)$/.exec(r.detail);
  return `<div class="dcoderen tool row${r.cls ?? ""}"${r.attrs ?? ""}><span class="ki">${KIND_ICONS[r.kind]}</span><span class="vb">${esc(r.verb)}</span>`
    + `<code class="tt" title="${esc(r.tip)}">${esc(r.target)}</code>`
    + `<span class="rm${r.bad ? " bad" : ""}">${r.since != null ? `<span data-since="${r.since}" title="Time since this window saw the tool running">${duration(Date.now() - r.since)}</span>` : counts ? `<span class="pl">+${counts[1]}</span><span class="mi">-${counts[2]}</span>` : esc(r.detail)}</span>`
    + `<span class="mk">${r.mark}</span>${r.after ?? ""}</div>`;
}

/// An answer with copy controls for code blocks and quotes.
export const answerHtml = (html: string) => html.replace(/<pre(\s[^>]*)?>/g, (_m, attrs = "") => `<pre class="dcode"${attrs}><button class="dcopy" data-copy title="Copy">Copy</button>`)
  .replace(/<\/blockquote>/g, () => `</blockquote><button type="button" class="dcopy dquote-copy" data-copy aria-label="Copy quote" title="Copy quote">${COPY_ICON}</button>`);

/// Copies the code block or quote containing the button.
export function copyCode(btn: HTMLElement) {
  const parent = btn.classList.contains("dquote-copy") ? btn.previousElementSibling : btn.parentElement;
  if (!parent) return;

  const text = parent.tagName === "BLOCKQUOTE" ? quotedMarkdown(parent) : parent.querySelector("code")?.textContent ?? "";
  const copying = btn.classList.contains("dquote-copy") ? writeClipboard(text).then(() => {
    btn.innerHTML = ICON.check;
    btn.setAttribute("aria-label", "Copied");
    btn.title = "Copied";
    setTimeout(() => {
      if (!btn.isConnected) return;

      btn.innerHTML = COPY_ICON;
      btn.setAttribute("aria-label", "Copy quote");
      btn.title = "Copy quote";
    }, 1200);
  }) : copy(text, btn);
  copying.catch(error => {
    console.warn(error);
    btn.textContent = "Copy failed";
  });
}
