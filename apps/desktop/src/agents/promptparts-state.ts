// A prompt entry split as the node tags it (crates/node/src/agents/tags.rs):
// what Beep told the agent in <beep-context>, what another session sent in
// <from-session id=".." name="..">, what a child it started came to in
// <beep-child id=".." name=".." state="finished|asking">, and the user's own
// words around them.

export type PromptPart =
  | { kind: "user"; text: string }
  | { kind: "context"; text: string }
  | { kind: "session"; id: string; name: string; text: string }
  | { kind: "child"; id: string; name: string; state: string; text: string };

const OPEN = /<(beep-context|from-session|beep-child)([ >])/g;

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
        : { kind: "session", id: attribute(head, "id"), name: attribute(head, "name"), text: inner });
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

/// Fold standalone file-reference paragraphs without changing the stored prompt.
export function compactPromptAttachments(html: string): string {
  if (!html.includes("@")) return html;
  const template = document.createElement("template");
  template.innerHTML = html;

  for (const paragraph of template.content.querySelectorAll("p")) {
    if (paragraph.closest("pre, code, details, blockquote") || paragraph.querySelector("code, img")) continue;
    const text = paragraph.textContent?.trim() ?? "";
    const references = [...text.matchAll(/@(?:"([^"\n]+)"|([^\s"]+))/g)];
    if (!references.length || text.replace(/@(?:"[^"\n]+"|[^\s"]+)/g, "").trim()) continue;
    const paths = references.map(match => match[1] ?? match[2]);
    if (paths.some(path => !/[\\/.]/.test(path))) continue;

    const items = document.createElement("div");
    items.className = "attachment-items";

    for (const path of paths) {
      const chip = document.createElement("button");
      chip.type = "button";
      chip.dataset.attachmentPath = path;
      chip.className = "attachment-file";
      chip.title = path;
      chip.textContent = path.split(/[\\/]/).filter(Boolean).pop() ?? path;
      items.append(chip);
    }

    if (paths.length > 5) {
      const batch = document.createElement("details");
      batch.className = "attachment-batch";
      const summary = document.createElement("summary");
      summary.textContent = `${paths.length} attachments`;
      batch.append(summary, items);
      paragraph.replaceWith(batch);
    } else {
      paragraph.replaceWith(items);
    }
  }

  return template.innerHTML;
}
