// A prompt entry split as the node tags it (crates/node/src/agents/tags.rs):
// what Beep told the agent in <beep-context>, what another session sent in
// <from-session id=".." name="..">, what a child it started came to in
// <beep-child id=".." name=".." state="finished|asking">, and the user's own
// words around them. Claude Code's own messages from another session
// (<cross-session-message from=".." from-name="..">) or one of its agents
// (<agent-message from="..">) count as another session's.

export type PromptPart =
  | { kind: "user"; text: string }
  | { kind: "context"; text: string }
  | { kind: "session"; id: string; name: string; text: string }
  | { kind: "child"; id: string; name: string; state: string; text: string };

const OPEN = /<(beep-context|from-session|beep-child|cross-session-message|agent-message)([ >])/g;

const unescape = (s: string) => s.replace(/&quot;/g, '"').replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");

function attribute(head: string, name: string) {
  const m = head.match(new RegExp(` ${name}="([^"]*)"`));
  return m ? unescape(m[1]) : "";
}

export function promptParts(text: string): PromptPart[] {
  const parts: PromptPart[] = [];
  const user = (s: string) => { const t = s.trim(); if (t) parts.push({ kind: "user", text: t }); };
  let rest = text;

  for (;;) {
    OPEN.lastIndex = 0;
    const m = OPEN.exec(rest);
    if (!m) { user(rest); return parts; }
    user(rest.slice(0, m.index));
    const tag = m[1];
    const headEnd = rest.indexOf(">", m.index);
    if (headEnd < 0) { user(rest.slice(m.index)); return parts; }
    const head = rest.slice(m.index + 1 + tag.length, headEnd);
    const body = rest.slice(headEnd + 1);
    const close = `</${tag}>`;
    const end = body.indexOf(close);
    const inner = (end < 0 ? body : body.slice(0, end)).trim();
    rest = end < 0 ? "" : body.slice(end + close.length);
    parts.push(tag === "beep-context"
      ? { kind: "context", text: inner }
      : tag === "beep-child"
        ? { kind: "child", id: attribute(head, "id"), name: attribute(head, "name"), state: attribute(head, "state"), text: inner }
        : tag === "from-session"
          ? { kind: "session", id: attribute(head, "id"), name: attribute(head, "name"), text: inner }
          : { kind: "session", id: attribute(head, "from"), name: attribute(head, "from-name"), text: inner });
  }
}

// Some imported session titles were taken from the first tagged prompt.
export function sessionMessageTitle(name: string): string {
  const title = name.trim();
  if (!/^<(from-session|beep-child|beep-context)([ >]|$)/.test(title)) return title;

  const part = promptParts(title)[0];
  if (part?.kind === "session" || part?.kind === "child") {
    const nested = part.name.trim();
    return nested.startsWith("<") ? "" : nested;
  }

  return "";
}

const PICTURE = /\.(?:png|jpe?g|gif|webp|heic|bmp|tiff?)$/i;

/// A prompt's Markdown HTML without its standalone file-reference
/// paragraphs, and the files they named. The stored prompt is unchanged.
export function promptAttachments(html: string): { html: string; files: string[] } {
  if (!html.includes("@")) return { html, files: [] };

  const template = document.createElement("template");
  template.innerHTML = html;
  const files: string[] = [];

  for (const paragraph of template.content.querySelectorAll("p")) {
    if (paragraph.closest("pre, code, details, blockquote") || paragraph.querySelector("code, img")) continue;
    const text = paragraph.textContent?.trim() ?? "";
    const references = [...text.matchAll(/@(?:"([^"\n]+)"|([^\s"]+))/g)];
    if (!references.length || text.replace(/@(?:"[^"\n]+"|[^\s"]+)/g, "").trim()) continue;
    const named = references.map(match => match[1] ?? match[2]);
    if (named.some(path => !/[\\/.]/.test(path))) continue;
    files.push(...named);
    paragraph.remove();
  }

  return { html: template.innerHTML, files };
}

/// Only pair pictures with file references when every picture has a path.
/// Otherwise keep the files visible instead of guessing which one to open.
export function attachmentPictures(files: string[], count: number): string[] {
  const pictures = files.filter(path => PICTURE.test(path));
  return pictures.length === count ? pictures : [];
}
