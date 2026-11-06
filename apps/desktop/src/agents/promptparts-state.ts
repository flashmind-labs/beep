// A prompt entry split as the node tags it (crates/node/src/agents/tags.rs):
// what Beep told the agent in <beep-context>, what another session sent in
// <from-session id=".." name="..">, and the user's own words around them.

export type PromptPart =
  | { kind: "user"; text: string }
  | { kind: "context"; text: string }
  | { kind: "session"; id: string; name: string; text: string };

const OPEN = /<(beep-context|from-session)([ >])/g;

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
      : { kind: "session", id: attribute(head, "id"), name: attribute(head, "name"), text: inner });
  }
}
