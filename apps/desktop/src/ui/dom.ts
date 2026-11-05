// ui/: helpers every window shares: Tauri bindings, DOM builders,
// formatting, menus, skins and banners. Main file: this one.
// Building elements: `el` and `button`, and the types views take them as.

/// A child for `el`: a node, text, or an empty slot, which is left out.
export type Kid = Node | string | null | undefined | false;

/// An element with `cls`; string children become text, never markup.
export function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, ...kids: Kid[]): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  for (const k of kids) if (k != null && k !== false) e.append(k);
  return e;
}

/// `b` marked pressed or not, for a chip or a segment of a choice.
export function press<B extends HTMLElement>(b: B, on: boolean) {
  b.setAttribute("aria-pressed", String(on));
  return b;
}

/// How the window's `h` makes elements, as the views it hands it to call it.
export type MakeEl = typeof el;

/// A button with `cls` that runs `onClick`.
export function button(label: string | Node, cls: string, onClick: (e: MouseEvent) => void) {
  const b = el("button", cls, label);
  b.type = "button";
  b.addEventListener("click", onClick);
  return b;
}

/// How the window makes its buttons (`dbtn` and `cls`), as the views it hands
/// them to call it.
export type MakeButton = (label: string | Node, cls: string, on: (e: MouseEvent) => void) => HTMLButtonElement;

/// "12 min" or "now": how long ago a time in milliseconds was, as the window
/// says it.
export type Ago = (ms: number) => string;

/// The first element of `html`.
export function h<T extends Element = HTMLElement>(html: string): T {
  const t = document.createElement("template");
  t.innerHTML = html.trim();
  return t.content.firstElementChild as T;
}

/// Children for `replaceChildren` or `append` without the empty slots a
/// conditional leaves (`null`, `undefined`, `false`), which the DOM would
/// print as text.
export function present<T>(kids: readonly (T | null | undefined | false)[] | null | undefined) {
  return (kids || []).filter((k): k is T => k != null && k !== false);
}
