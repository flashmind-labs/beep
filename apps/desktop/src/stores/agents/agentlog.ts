// A coding agent's tool entries, read back from the text the node writes.

import { MEDIA_EXTENSIONS } from "../agentpane/media-state.ts";
import { codeFile } from "../agentpane/files-state.ts";
import { findLinkedSession, parseSessionRef } from "./nesting-state.ts";
import { bytes } from "../../ui/format.ts";

const STATUSES = ["pending", "in_progress", "completed", "failed"];
const LIVE = new RegExp(`^([\\s\\S]*?) \\[(${STATUSES.join("|")})\\](?:\\n([\\s\\S]*))?$`);

/// The parts of a tool entry: its title, its status (one of `STATUSES`, or
/// "" when the text does not say) and its output, "" when it has none.
interface ToolParts {
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
  const text = title.trim();
  if (text.startsWith("Beep: ")) return text;
  const [first, ...rest] = text.split("\n");
  return [first.replace(/`([^`\n]*)`/g, "$1"), ...rest].join("\n");
}

/// Output the agent wrapped in one fenced block, without the fence.
function unfence(output: string) {
  const m = /^(`{3,}|~{3,})[^\n`]*\n([\s\S]*?)\n?\1\s*$/.exec(output.trim());
  return (m ? m[2] : output).replace(/\s+$/, "");
}

const IMAGE_LINE = /^\[image: (.+)\]$/;

/// A message as the log holds it: its words, then a line for each image sent with it.
export function sentText(text: string, images: readonly { name: string }[]) {
  return [text, ...images.map(image => `[image: ${image.name}]`)].filter(Boolean).join("\n");
}

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
const TAIL_LINES = 5;
/// The characters one output line holds before it wraps, about.
const WRAP_AT = 100;

/// The visual lines a line of output takes when it wraps at `WRAP_AT`.
const rows = (line: string) => Math.max(1, Math.ceil(line.length / WRAP_AT));

/// The end of a step's output as it shows when opened: its last `TAIL_LINES`
/// lines as they wrap, so one long line of JSON counts as the many it shows as.
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
/// `exited with code 2`, `Exit status 2`, `status 3`, `exit 1`), or null. The
/// last one wins by default, as a command's own code ends its output; `first`
/// takes the first, for a row that shows the code the failure began with.
export function exitCode(output: string, first = false) {
  const all = [...output.matchAll(/\b(?:exit(?:ed with)?(?:\s+(?:code|status))?|status)\s*:?\s*(-?\d+)\b/gi)];
  const hit = first ? all[0] : all[all.length - 1];
  return hit ? Number(hit[1]) : null;
}

/// How far a running file send has gone, from the `[sent] <done> <total>`
/// line the node writes first in its output ("412 MB of 1.2 GB"), and the
/// output without it. `sent` is null for any other call.
export function sentFiles(output: string): { sent: string | null; output: string } {
  const m = /^\[sent\] (\d+) (\d+)(?:\n|$)/.exec(output);
  if (!m) return { sent: null, output };
  return { sent: `${bytes(Number(m[1]))} of ${bytes(Number(m[2]))}`, output: output.slice(m[0].length) };
}

/// A step of a subagent, as the node writes it on the call that started it.
interface SubStep {
  status: string;
  title: string;
}

/// A subagent's call, read from its output: the node writes `[steps] N`
/// first, then `[step <status>] <title>` for each of the last steps, then
/// `[agent] <name>` when it knows its type or custom agent's name, then
/// `[session] <id>` for a native conversation, or, for
/// an agent another agent started, `[child] <id>@<machine> <state> <key>`
/// (the session to open), then `[model] <model>` when known, before the
/// answer. Null for any other call.
export function subagent(output: string): { count: number; steps: SubStep[]; result: string; model: string | null; agent?: string; child?: SubChild; session?: string } | null {
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
  const session = /^\[session\] (subagent:\S+)$/.exec(lines[i] ?? "");
  if (session) i++;
  const kid = /^\[child\] (\S+) (\S+)(?: (\S+))?$/.exec(lines[i] ?? "");
  if (kid) i++;
  // What the child's own session spent: shown on the card, never added to the parent's.
  const spend = kid ? /^\[spend\] (\d+) (\S+)$/.exec(lines[i] ?? "") : null;
  if (spend) i++;
  const model = /^\[model\] (.+)$/.exec(lines[i] ?? "");
  if (model) i++;
  const result = lines.slice(i).join("\n").trim();
  const out: { count: number; steps: SubStep[]; result: string; model: string | null; agent?: string; child?: SubChild; session?: string } = { count: Number(head[1]), steps, result, model: model?.[1].trim() || null };
  if (session) out.session = session[1];
  if (named) out.agent = named[1].trim();
  if (kid) out.child = { id: kid[1], state: kid[2], key: kid[3] ?? "" };
  if (kid && spend) out.child = { ...out.child!, tokens: Number(spend[1]), cost: spend[2] === "-" ? null : Number(spend[2]) };
  return out;
}

/// The session an agent started, as its card in the parent's log names it.
interface SubChild {
  /// `<session>@<machine>`.
  id: string;
  /// working, waiting_on_user, idle or ended.
  state: string;
  /// The id its machine lists it under; empty when the node did not say.
  key: string;
  /// What its session spent, when the node said: tokens and US dollars (null when unpriced).
  tokens?: number;
  cost?: number | null;
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

/// `s` escaped for a quoted HTML attribute.
const attr = (s: string) => s.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;");

/// Agent warnings, Claude API errors and Antigravity's ⚠ notices (such as
/// its quota message) use the warning color.
export function markWarnings(html: string) {
  return html.replace(/<p>(?=(?:Warning: |API Error: |\u26a0\ufe0f?\s))/g, '<p class="agwarn">');
}

/// The agent's reply as HTML with each media file it names (relative to its
/// folder or absolute) as a link.
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

/// The reply's inline code that names a file (`codeFile`), such as
/// `src/main.rs:12`, marked for the pane to open; code blocks stay as they are.
export function linkCodeFiles(html: string) {
  return html.replace(/(?<!<pre>)<code>([^<&]+)<\/code>/g, (whole, text: string) =>
    codeFile(text) ? `<code class="fref" data-file="${attr(text)}" role="link" tabindex="0">${text}</code>` : whole);
}

/// Inline code in a reply that matches another known session's id (or `<id>@<machine>`),
/// marked to open that session on click; code blocks and existing links stay as they are.
export function linkCodeSessions(
  html: string,
  rows: readonly { id: string; machine_id: string; machine_name: string; title?: string | null }[],
  current?: { machine_id: string; id: string } | null,
) {
  if (!rows.length || !html.includes("<code>")) return html;
  let inLink = 0, inPre = 0;
  const parts = html.split(/(<[^>]+>)/);

  for (let i = 0; i < parts.length; i++) {
    const part = parts[i];
    if (part.startsWith("<")) {
      if (/^<a\b/i.test(part)) inLink++;
      else if (/^<\/a>/i.test(part)) inLink = Math.max(0, inLink - 1);
      else if (/^<pre\b/i.test(part)) inPre++;
      else if (/^<\/pre>/i.test(part)) inPre = Math.max(0, inPre - 1);
      else if (!inLink && !inPre && part === "<code>" && i + 2 < parts.length && parts[i + 2] === "</code>") {
        const text = parts[i + 1];
        const ref = !text.includes("&") ? parseSessionRef(text) : null;
        const hit = ref && findLinkedSession(rows, ref, current?.machine_id);
        if (hit && !(current && hit.machine_id === current.machine_id && hit.id === current.id)) {
          const title = hit.title ? `Open ${hit.title}` : "Open session";
          parts[i] = `<code class="fref" data-session="${attr(text)}" title="${attr(title)}" role="link" tabindex="0">`;
        }
      }
    }
  }

  return parts.join("");
}

/// The media files a line of text names, each once, in order: for the
/// buttons on a tool's row.
export function mediaPaths(text: string) {
  const found: string[] = [];
  for (const m of text.matchAll(MEDIA_PATH)) if (!found.includes(m[2])) found.push(m[2]);
  return found;
}
