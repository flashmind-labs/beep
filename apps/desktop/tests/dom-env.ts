// The bar-env stub with a working DOM: `#bar` is an element that parses
// HTML, finds by simple selectors and moves nodes as a webview does, so a
// test can draw a chat and check which elements stayed. Import it before any
// `src/bar` module, in place of bar-env. `calls` records every `invoke`,
// `answers` holds what a command returns (null when absent), and `removed`
// gets every element taken out of the page.

export const calls: { cmd: string; args: unknown }[] = [];
export const answers: Record<string, unknown> = {};
export const removed = new Set<object>();

const VOID = new Set(["area", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"]);
const ENTITIES: Record<string, string> = { amp: "&", lt: "<", gt: ">", quot: '"', "#39": "'", apos: "'", nbsp: " " };
const decode = (s: string) => s.replace(/&(#?\w+);/g, (m, e: string) => ENTITIES[e] ?? m);
const escText = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
const escAttr = (s: string) => escText(s).replace(/"/g, "&quot;");

class FakeNode {
  parentNode: FakeElement | null = null;
  get parentElement(): FakeElement | null { return this.parentNode instanceof FakeElement && this.parentNode.tagName !== "#fragment" ? this.parentNode : null; }
  get isConnected(): boolean {
    let n: FakeNode | null = this;
    while (n.parentNode) n = n.parentNode;
    return n === root;
  }
  get nextSibling(): FakeNode | null {
    const p = this.parentNode;
    return p ? p.childNodes[p.childNodes.indexOf(this) + 1] ?? null : null;
  }
  get textContent(): string { return ""; }
  remove() {
    const p = this.parentNode;
    if (!p) return;
    if (this.isConnected) removed.add(this);
    p.childNodes.splice(p.childNodes.indexOf(this), 1);
    this.parentNode = null;
  }
  before(...nodes: (FakeNode | string)[]) { this.parentNode?.insert(nodes, this); }
  after(...nodes: (FakeNode | string)[]) { this.parentNode?.insert(nodes, this.nextSibling); }
  replaceWith(...nodes: (FakeNode | string)[]) {
    const p = this.parentNode;
    if (!p) return;
    const at = this.nextSibling;
    this.remove();
    p.insert(nodes, at);
  }
  serialize(): string { return ""; }
}

class FakeText extends FakeNode {
  data: string;
  constructor(data: string) { super(); this.data = data; }
  override get textContent() { return this.data; }
  override serialize() { return escText(this.data); }
}

class ClassList {
  private el: FakeElement;
  constructor(el: FakeElement) { this.el = el; }
  private get list() { return this.el.className.split(/\s+/).filter(Boolean); }
  contains(c: string) { return this.list.includes(c); }
  add(...cs: string[]) { this.el.className = [...new Set([...this.list, ...cs])].join(" "); }
  remove(...cs: string[]) { this.el.className = this.list.filter(c => !cs.includes(c)).join(" "); }
  toggle(c: string, on?: boolean) {
    const want = on ?? !this.contains(c);
    want ? this.add(c) : this.remove(c);
    return want;
  }
}

class FakeElement extends FakeNode {
  childNodes: FakeNode[] = [];
  attrs = new Map<string, string>();
  style: Record<string, unknown> = { setProperty() {}, getPropertyValue: () => "" };
  classList = new ClassList(this);
  scrollTop = 0;
  scrollHeight = 0;
  clientHeight = 0;
  offsetTop = 0;
  offsetHeight = 0;
  value = "";
  placeholder = "";
  dataset: Record<string, string>;
  tagName: string;
  constructor(tagName: string) {
    super();
    this.tagName = tagName;
    const attrs = this.attrs;
    const key = (k: string) => "data-" + k.replace(/[A-Z]/g, c => "-" + c.toLowerCase());
    this.dataset = new Proxy({}, {
      get: (_t, k: string) => attrs.get(key(k)),
      set: (_t, k: string, v) => { attrs.set(key(k), String(v)); return true; },
      has: (_t, k: string) => attrs.has(key(k)),
    });
  }
  get content() { return this; }
  get disabled() { return this.attrs.has("disabled"); }
  set disabled(v: boolean) { v ? this.attrs.set("disabled", "") : this.attrs.delete("disabled"); }
  get id() { return this.attrs.get("id") ?? ""; }
  get className() { return this.attrs.get("class") ?? ""; }
  set className(v: string) { this.attrs.set("class", v); }
  get hidden() { return this.attrs.has("hidden"); }
  set hidden(v: boolean) { v ? this.attrs.set("hidden", "") : this.attrs.delete("hidden"); }
  getAttribute(k: string) { return this.attrs.get(k) ?? null; }
  setAttribute(k: string, v: string) { this.attrs.set(k, String(v)); }
  removeAttribute(k: string) { this.attrs.delete(k); }
  hasAttribute(k: string) { return this.attrs.has(k); }
  get children() { return this.childNodes.filter((n): n is FakeElement => n instanceof FakeElement); }
  get firstElementChild(): FakeElement | null { return this.children[0] ?? null; }
  override get textContent(): string { return this.childNodes.map(n => n.textContent).join(""); }
  override set textContent(v: string) { this.clear(); if (v) this.insert([v], null); }
  get innerText() { return this.textContent; }
  get innerHTML() { return this.childNodes.map(n => n.serialize()).join(""); }
  set innerHTML(html: string) { this.clear(); this.insert(parse(html), null); }
  get outerHTML() { return this.serialize(); }
  override serialize(): string {
    const attrs = [...this.attrs].map(([k, v]) => (v === "" ? ` ${k}` : ` ${k}="${escAttr(v)}"`)).join("");
    return VOID.has(this.tagName) ? `<${this.tagName}${attrs}>` : `<${this.tagName}${attrs}>${this.innerHTML}</${this.tagName}>`;
  }
  private clear() { for (const n of [...this.childNodes]) n.remove(); }
  insert(nodes: (FakeNode | string)[], at: FakeNode | null) {
    for (const x of nodes) {
      const n = typeof x === "string" ? new FakeText(x) : x;
      n.remove();
      const i = at ? this.childNodes.indexOf(at) : -1;
      i < 0 ? this.childNodes.push(n) : this.childNodes.splice(i, 0, n);
      n.parentNode = this;
    }
  }
  append(...nodes: (FakeNode | string)[]) { this.insert(nodes, null); }
  appendChild<T extends FakeNode>(n: T) { this.insert([n], null); return n; }
  prepend(...nodes: (FakeNode | string)[]) { this.insert(nodes, this.childNodes[0] ?? null); }
  insertBefore<T extends FakeNode>(n: T, at: FakeNode | null) { this.insert([n], at); return n; }
  replaceChildren(...nodes: (FakeNode | string)[]) { this.clear(); this.insert(nodes, null); }
  contains(n: FakeNode | null): boolean {
    for (let x = n; x; x = x.parentNode) if (x === this) return true;
    return false;
  }
  descendants(): FakeElement[] { return this.children.flatMap(c => [c, ...c.descendants()]); }
  querySelectorAll(sel: string) { return this.descendants().filter(el => matches(el, sel, this)); }
  querySelector(sel: string) { return this.descendants().find(el => matches(el, sel, this)) ?? null; }
  matches(sel: string) { return matches(this, sel, null); }
  closest(sel: string) {
    for (let x: FakeElement | null = this; x; x = x.parentElement) if (matches(x, sel, null)) return x;
    return null;
  }
  addEventListener() {}
  removeEventListener() {}
  focus() {}
  blur() {}
  scrollTo() {}
  scrollIntoView() {}
  setPointerCapture() {}
  getBoundingClientRect() { return { top: 0, left: 0, right: 0, bottom: 0, width: 0, height: 0 }; }
}

/// Parses `html` into nodes. Enough for the markup the bar writes.
function parse(html: string): FakeNode[] {
  const top = new FakeElement("#fragment");
  let cur = top;
  const re = /<!--[\s\S]*?-->|<\/([\w-]+)\s*>|<([\w-]+)((?:\s+[^\s=>/]+(?:\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+))?)*)\s*(\/?)>|([^<]+|<)/g;
  for (const m of html.matchAll(re)) {
    const [, close, open, attrs, self, text] = m;
    if (text !== undefined) cur.append(new FakeText(decode(text)));
    else if (close) {
      for (let x: FakeElement | null = cur; x && x !== top; x = x.parentNode) {
        if (x.tagName === close.toLowerCase()) { cur = x.parentNode!; break; }
      }
    } else if (open) {
      const el = new FakeElement(open.toLowerCase());
      for (const a of (attrs ?? "").matchAll(/([^\s=>/]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+)))?/g)) {
        el.attrs.set(a[1].toLowerCase(), decode(a[2] ?? a[3] ?? a[4] ?? ""));
      }
      cur.append(el);
      if (!self && !VOID.has(el.tagName)) cur = el;
    }
  }
  const nodes = [...top.childNodes];
  for (const n of nodes) n.parentNode = null;
  return nodes;
}

/// Whether `el` matches selector list `sel`. Supports tags, `#id`,
/// `.class`, `[attr]`, `[attr="v"]`, `:scope`, `:disabled`, descendant and `>`.
function matches(el: FakeElement, sel: string, scope: FakeElement | null): boolean {
  return sel.split(",").some(part => matchChain(el, part.trim().replace(/\s*>\s*/g, " > ").split(/\s+/), scope));
}
function matchChain(el: FakeElement, parts: string[], scope: FakeElement | null): boolean {
  const last = parts.at(-1)!;
  if (!matchCompound(el, last, scope)) return false;
  if (parts.length === 1) return true;
  const rest = parts.slice(0, -1);
  if (rest.at(-1) === ">") {
    const p = el.parentElement;
    return !!p && matchChain(p, rest.slice(0, -1), scope);
  }
  for (let p = el.parentElement; p; p = p.parentElement) if (matchChain(p, rest, scope)) return true;
  return false;
}
function matchCompound(el: FakeElement, sel: string, scope: FakeElement | null): boolean {
  for (const [tok] of sel.matchAll(/:scope|:disabled|#[\w-]+|\.[\w-]+|\[[^\]]+\]|[\w-]+|\*/g)) {
    if (tok === ":scope") { if (el !== scope) return false; }
    else if (tok === ":disabled") { if (!el.hasAttribute("disabled")) return false; }
    else if (tok === "*") continue;
    else if (tok[0] === "#") { if (el.id !== tok.slice(1)) return false; }
    else if (tok[0] === ".") { if (!el.classList.contains(tok.slice(1))) return false; }
    else if (tok[0] === "[") {
      const m = /^\[([\w-]+)(?:="?([^"\]]*)"?)?\]$/.exec(tok)!;
      if (!el.hasAttribute(m[1]) || (m[2] !== undefined && el.getAttribute(m[1]) !== m[2])) return false;
    } else if (el.tagName !== tok.toLowerCase()) return false;
  }
  return true;
}

const root = new FakeElement("html");
export const bar = new FakeElement("div");
bar.setAttribute("id", "bar");
root.append(bar);

const g = globalThis as Record<string, unknown>;
g.Node = FakeNode;
g.Element = FakeElement;
g.HTMLElement = FakeElement;
g.document = {
  documentElement: root,
  body: root,
  getElementById: (id: string) => root.querySelector(`#${id}`),
  querySelector: (sel: string) => root.querySelector(sel),
  querySelectorAll: (sel: string) => root.querySelectorAll(sel),
  addEventListener() {},
  createElement: (tag: string) => new FakeElement(tag),
};
g.window = globalThis;
// Frames wait until a test runs them.
const frames = new Map<number, () => void>();
let frameId = 0;
g.requestAnimationFrame = (cb: () => void) => { frames.set(++frameId, cb); return frameId; };
g.cancelAnimationFrame = (id: number) => { frames.delete(id); };
export function runFrames() {
  const due = [...frames.values()];
  frames.clear();
  for (const cb of due) cb();
}
g.addEventListener = () => {};
g.ResizeObserver = class { observe() {} unobserve() {} };
g.MutationObserver = class { observe() {} };
g.HTMLTextAreaElement = FakeElement;
g.__TAURI__ = {
  core: { invoke: async (cmd: string, args: unknown) => { calls.push({ cmd, args }); return answers[cmd] ?? null; }, Channel: class {} },
  event: { listen: async () => () => {} },
};
