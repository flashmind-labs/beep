// A coding agent's tool entries, read back from the text the node writes.
// A live session writes `title [status]` and then the output's lines; a
// session Beep follows from Claude Code's transcript writes `Name: detail`,
// with ` (failed)` after one that failed.

import { MEDIA_EXTENSIONS } from "./media-state.ts";

const STATUSES = ["pending", "in_progress", "completed", "failed"];
const LIVE = new RegExp(`^([\\s\\S]*?) \\[(${STATUSES.join("|")})\\](?:\\n([\\s\\S]*))?$`);

/// The parts of a tool entry: its title, its status (one of `STATUSES`, or
/// "" when the text does not say) and its output, "" when it has none.
export interface ToolParts {
  title: string;
  status: string;
  output: string;
}

/// Reads a tool entry's text, which may be missing, into its parts.
export function parseTool(entry: unknown): ToolParts {
  const text = String(entry ?? "");
  const live = LIVE.exec(text);
  if (live) return { title: plainTitle(live[1]), status: live[2], output: unfence(live[3] ?? "") };
  const failed = / \(failed\)$/.exec(text);
  if (failed) return { title: text.slice(0, failed.index), status: "failed", output: "" };
  return { title: text, status: "", output: "" };
}

/// Titles quote what they name in backticks (Edit `src/main.rs`); the row
/// shows them in mono already.
function plainTitle(title: string) {
  return title.trim().replace(/`([^`\n]*)`/g, "$1");
}

/// Output the agent wrapped in one fenced block, without the fence.
function unfence(output: string) {
  const m = /^(`{3,}|~{3,})[^\n`]*\n([\s\S]*?)\n?\1\s*$/.exec(output.trim());
  return (m ? m[2] : output).replace(/\s+$/, "");
}

const PASTE = /<pasted_content id="([^"]*)">\n?([\s\S]*?)\n?<\/pasted_content id="\1">/g;

/// A user message as HTML, with each block a terminal pasted in
/// (`<pasted_content id="…">…</pasted_content id="…">`) folded into
/// "Pasted text · 8 lines", opened on click. `esc` escapes text; `open(i)`
/// says whether the i-th block was left open, so a redraw keeps it.
export function userHtml(text: string, esc: (s: string) => string, open: (i: number) => boolean = () => false) {
  let html = "", last = 0, i = 0;
  for (const m of text.matchAll(PASTE)) {
    const body = m[2] ?? "";
    html += esc(text.slice(last, m.index));
    const lines = body.split("\n").length;
    html += `<details class="paste" data-paste="${i}"${open(i++) ? " open" : ""}><summary>Pasted text · ${lines === 1 ? "1 line" : `${lines} lines`}</summary><pre>${esc(body)}</pre></details>`;
    last = m.index + m[0].length;
  }
  return html + esc(text.slice(last));
}

const IMAGE_LINE = /^\[image: (.+)\]$/;

/// An agent or tool entry's text without its image lines, which the node
/// writes for each picture the agent showed (`[image: image-<key>]`), and
/// the names those lines give, in order.
export function imageLines(text: string) {
  const kept: string[] = [], names: string[] = [];
  for (const line of String(text).split("\n")) {
    const m = IMAGE_LINE.exec(line.trim());
    if (m) names.push(m[1]);
    else kept.push(line);
  }
  return { text: kept.join("\n").replace(/\s+$/, ""), names };
}

/// How many lines of a step's output show when it is opened, before
/// "Show all", counted as they wrap.
export const TAIL_LINES = 5;
/// The characters one output line holds before it wraps, about.
export const WRAP_AT = 100;

/// The visual lines a line of output takes when it wraps at `WRAP_AT`.
const rows = (line: string) => Math.max(1, Math.ceil(line.length / WRAP_AT));

/// The end of a step's output as it shows when opened: its last
/// `TAIL_LINES` lines as they wrap, so one long line of JSON counts as the
/// many it shows as. `total` is the output's lines, for "Show all N lines";
/// `cut` whether anything is left out.
export function tailLines(output: string, keep = TAIL_LINES) {
  if (!output) return { shown: "", total: 0, cut: false };
  const lines = output.split("\n"), shown: string[] = [];
  let used = 0;
  for (let i = lines.length - 1; i >= 0 && used < keep; i--) {
    const line = lines[i], room = keep - used;
    if (rows(line) <= room) { shown.unshift(line); used += rows(line); continue; }
    // A line longer than the room left shows its end.
    shown.unshift(`…${line.slice(line.length - room * WRAP_AT + 1)}`);
    used = keep;
  }
  const text = shown.join("\n");
  return { shown: text, total: lines.length, cut: text !== output };
}

/// The exit code a failed command's output names (`exit code: 101`,
/// `Exit status 2`, `exited with 1`), or null.
export function exitCode(output: string) {
  const all = [...output.matchAll(/\bexit(?:ed with|\s+code|\s+status)?\s*:?\s*(-?\d+)\b/gi)];
  return all.length ? Number(all[all.length - 1][1]) : null;
}

/// A step of a subagent, as the node writes it on the call that started it.
export interface SubStep {
  status: string;
  title: string;
}

/// A subagent's call, read from its output: the node writes `[steps] N`
/// first, then `[step <status>] <title>` for each of the last steps, then
/// `[agent] <name>` when it knows its type or custom agent's name, before
/// what the subagent answered. Null for any other call.
export function subagent(output: string): { count: number; steps: SubStep[]; result: string; agent?: string } | null {
  const lines = output.split("\n");
  const head = /^\[steps\] (\d+)$/.exec(lines[0] ?? "");
  if (!head) return null;
  const steps: SubStep[] = [];
  let i = 1;
  for (; i < lines.length; i++) {
    const m = /^\[step (pending|in_progress|completed|failed)\] (.*)$/.exec(lines[i]);
    if (!m) break;
    steps.push({ status: m[1], title: m[2] });
  }
  const named = /^\[agent\] (.+)$/.exec(lines[i] ?? "");
  if (named) i++;
  const result = lines.slice(i).join("\n").trim();
  return named ? { count: Number(head[1]), steps, result, agent: named[1].trim() } : { count: Number(head[1]), steps, result };
}

/// One question Codex asked and the user's answer, as its echo of the
/// answer carries them.
export interface Answered {
  question: string;
  answer: string;
}

const REPLY = /^\s*<send_user_message_question_reply>([\s\S]*)<\/send_user_message_question_reply>\s*$/;

/// The questions and answers in a user message that is Codex's echo of
/// answered questions (`<send_user_message_question_reply>[{…}]</…>`), or
/// null when it is not one or its JSON does not read.
export function answeredQuestions(text: string): Answered[] | null {
  const m = REPLY.exec(text);
  if (!m) return null;
  try {
    const items: unknown = JSON.parse(m[1]);
    if (!Array.isArray(items) || !items.length) return null;
    const out: Answered[] = [];
    for (const it of items) {
      if (!it || typeof it !== "object") return null;
      const { question, answer } = it as Record<string, unknown>;
      if (typeof answer !== "string") return null;
      out.push({ question: typeof question === "string" ? question : "", answer });
    }
    return out;
  } catch {
    return null;
  }
}

/// Pictures the node shows as a reply's own, numbered in the order named.
const PICTURE = /\.(?:png|jpe?g|gif|webp)$/i;
const MEDIA_PATH = new RegExp(`(^|[\\s(])((?:~|\\.{1,2})?\\/?[\\w@%+-][\\w@%+.\\/-]*\\.(?:${MEDIA_EXTENSIONS.join("|")}))(?=$|[\\s).,;:!?])`, "gi");

const attr = (s: string) => s.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;");

/// The agent's reply as HTML with each media file it names (relative to
/// its folder or absolute) as a link. Pictures are numbered in the order
/// first named, as the node shows those files as the reply's pictures in
/// that order; every link carries its path, for the media pane. Text
/// inside links and code blocks is left alone.
export function linkImagePaths(html: string) {
  const paths: string[] = [];
  let skip = 0;
  const out = html.split(/(<[^>]+>)/).map(part => {
    if (part.startsWith("<")) {
      if (/^<(a|pre)\b/i.test(part)) skip++;
      else if (/^<\/(a|pre)>/i.test(part)) skip = Math.max(0, skip - 1);
      return part;
    }
    if (skip) return part;
    return part.replace(MEDIA_PATH, (_, lead: string, path: string) => {
      let img = "";
      if (PICTURE.test(path)) {
        let i = paths.indexOf(path);
        if (i < 0) { i = paths.length; paths.push(path); }
        img = ` data-img="${i}"`;
      }
      return `${lead}<a class="imgl"${img} data-path="${attr(path)}" title="${attr(path)}">${path}</a>`;
    });
  });
  return { html: out.join(""), paths };
}

/// The media files a line of text names, each once, in order: for the
/// buttons on a tool's row.
export function mediaPaths(text: string) {
  const found: string[] = [];
  for (const m of text.matchAll(MEDIA_PATH)) if (!found.includes(m[2])) found.push(m[2]);
  return found;
}
