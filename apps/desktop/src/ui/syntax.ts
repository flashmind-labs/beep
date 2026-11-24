// Code coloured by the CodeMirror language parsers: the token classes the
// editor and the diff share, and lines split into coloured spans.
import { LanguageDescription, type Language } from "@codemirror/language";
import { languages } from "@codemirror/language-data";
import { highlightTree, tagHighlighter, tags as t } from "@lezer/highlight";

/// The token classes, and the names, types and markup the parser sees.
export const tokens = tagHighlighter([
  { tag: t.keyword, class: "k" },
  { tag: [t.string, t.special(t.string), t.regexp, t.character], class: "s" },
  { tag: t.comment, class: "c" },
  { tag: [t.number, t.bool, t.null, t.atom, t.constant(t.name), t.escape], class: "n" },
  { tag: [t.typeName, t.className, t.namespace, t.attributeName], class: "ty" },
  { tag: [t.function(t.variableName), t.function(t.propertyName), t.macroName, t.labelName], class: "fn" },
  { tag: [t.propertyName, t.self], class: "pr" },
  { tag: [t.tagName, t.meta, t.annotation], class: "tg" },
  { tag: t.heading, class: "hd" },
  { tag: [t.link, t.url], class: "lk" },
  { tag: t.emphasis, class: "em" },
  { tag: t.strong, class: "st" },
]);

/// A coloured run of a line, from and to as character offsets in it.
export interface Span { from: number; to: number; cls: string }

/// Colours lines parsed together as one text, giving each line its spans.
export type Colour = (lines: readonly string[]) => Span[][];

/// The colouring for file `path` once its parser has loaded, or null for a
/// language CodeMirror does not know.
export async function colourFor(path: string): Promise<Colour | null> {
  const found = LanguageDescription.matchFilename(languages, path);
  if (!found) return null;
  const { language } = await found.load();
  return lines => colourLines(language, lines);
}

function colourLines(language: Language, lines: readonly string[]): Span[][] {
  const starts: number[] = [], out: Span[][] = lines.map(() => []);
  let end = 0;

  for (const line of lines) {
    starts.push(end);
    end += line.length + 1;
  }

  let line = 0;

  // Spans come in order and may run over several lines, as a block comment does.
  highlightTree(language.parser.parse(lines.join("\n")), tokens, (from, to, cls) => {
    for (let at = from; at < to;) {
      while (line + 1 < starts.length && starts[line + 1] <= at) line++;
      const lineEnd = starts[line] + lines[line].length;
      if (lineEnd > at) out[line].push({ from: at - starts[line], to: Math.min(to, lineEnd) - starts[line], cls });
      at = lineEnd + 1;
    }
  });

  return out;
}
