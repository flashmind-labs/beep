// A command in a session log as colored HTML: the shell, and the body of
// each heredoc in the language of what reads it. Pure, so the tests load it;
// the colors come from the same simple highlighter the Files tab uses.
import { highlight, languageOf } from "../agentpane/files-state.ts";
import { esc } from "../ui/format.ts";

/// A run of whole lines and the language they are in ("" for plain text).
export interface Segment {
  lang: string;
  lines: string[];
}

const SEPARATOR = /\|\||&&|[|;&]/;

const INTERPRETERS: [RegExp, string][] = [
  [/^python[\d.]*$/, "Python"],
  [/^(node|nodejs|bun|deno|tsx|ts-node)$/, "JavaScript"],
  [/^ruby$/, "Ruby"],
  [/^(sh|bash|zsh|dash)$/, "Shell"],
];

/// The language of a heredoc body, from the command line it opens on: the
/// interpreter that reads it (`python3 - <<'PY'`), else the extension of the
/// file it is written to (`cat > app.tsx <<EOF`), else "".
export function heredocLang(before: string) {
  // The last command of a pipeline or list is the one that reads it.
  const words = (before.split(SEPARATOR).pop() ?? "").trim().split(/\s+/);
  for (const w of words) {
    const name = w.replace(/^["']|["']$/g, "").split("/").pop() ?? "";
    const hit = INTERPRETERS.find(([re]) => re.test(name));
    if (hit) return hit[1];
  }
  const file = /(?:^|\s)\d*>>?\s*(?:"([^"]+)"|'([^']+)'|(\S+))/.exec(before) ?? /\btee\s+(?:-a\s+)?(?:"([^"]+)"|'([^']+)'|(\S+))/.exec(before);
  const path = file ? (file[1] ?? file[2] ?? file[3]) : "";
  return path ? languageOf(path) : "";
}

/// The heredocs a shell line opens, in order: their end word, whether tabs
/// are stripped from its end line (`<<-`), and the language of the body.
function opened(line: string) {
  const out: { end: string; dash: boolean; lang: string }[] = [];
  for (const m of line.matchAll(/<<(-?)\s*(?:'([^']+)'|"([^"]+)"|\\?([A-Za-z_][\w]*))/g)) {
    // `<<<` is a here-string, not a heredoc.
    if (line[m.index! - 1] === "<" || line[m.index! + 2] === "<") continue;
    out.push({ end: m[2] ?? m[3] ?? m[4], dash: m[1] === "-", lang: heredocLang(`${line.slice(0, m.index)} ${line.slice(m.index! + m[0].length).split(SEPARATOR)[0]}`) });
  }
  return out;
}

/// The lines of `command` in runs of shell and heredoc bodies. The end
/// line of a heredoc is shell again.
export function segments(command: string): Segment[] {
  const out: Segment[] = [];
  const add = (lang: string, line: string) => {
    const last = out.at(-1);
    if (last && last.lang === lang) last.lines.push(line);
    else out.push({ lang, lines: [line] });
  };
  let pending: ReturnType<typeof opened> = [];
  for (const line of command.split("\n")) {
    const doc = pending[0];
    if (!doc) {
      add("Shell", line);
      pending = opened(line);
    } else if ((doc.dash ? line.replace(/^\t+/, "") : line) === doc.end) {
      add("Shell", line);
      pending.shift();
    } else add(doc.lang, line);
  }
  return out;
}

/// The most characters of a command that are colored; a longer one shows plain.
const MAX_CHARS = 20000;

/// A command as HTML, every character escaped.
export function commandHtml(command: string) {
  if (command.length > MAX_CHARS) return esc(command);
  return segments(command).flatMap(s => s.lines.map(l => highlight(l, s.lang))).join("\n");
}

const CACHE_MAX = 200;
const cache = new Map<string, string>();

/// `commandHtml`, kept by command text so a redraw does not color it again.
export function cachedCommandHtml(command: string) {
  let html = cache.get(command);
  if (html === undefined) {
    html = commandHtml(command);
    if (cache.size >= CACHE_MAX) cache.delete(cache.keys().next().value!);
    cache.set(command, html);
  }
  return html;
}
