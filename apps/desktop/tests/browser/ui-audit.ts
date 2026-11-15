// UI audit: measures every ui-baseline screen for near-miss alignment and
// runs axe-core on it.
//
// A difference of 0.5 to 4 CSS px between two things that look meant to
// match (an icon's left and top inset, the space above and below a label)
// is reported; a bigger one is taken as a design decision. Measurements use
// the drawn ink: the paths of an icon, a glyph's bounds from the font.
//
// Run from the repository root:
//   flock .artifacts/browser.lock sh -c 'npm --prefix apps/desktop run build && cd apps/desktop && UI_AUDIT=1 npx playwright test ui-baseline --project=macOS --workers=8 --reporter=line'
//   node --import ./apps/desktop/tests/ts.mjs apps/desktop/tests/browser/ui-audit.ts
// then open `.artifacts/ui-audit/report.html`.

import { mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import type { Page } from "@playwright/test";
import type * as Axe from "axe-core";

declare global {
  interface Window { axe: typeof Axe }
}

export const auditing = Boolean(process.env.UI_AUDIT);
const out = fileURLToPath(new URL("../../../../.artifacts/ui-audit/", import.meta.url));

type Box = { x: number; y: number; w: number; h: number };
type Mark = { x1: number; y1: number; x2: number; y2: number; label: string };
type Finding = { check: string; sig: string; what: string; delta: number; detail: string; box: Box; marks: Mark[] };
type Screen = { name: string; viewport: { width: number; height: number }; findings: Finding[]; small: Record<string, number> };

/// Measures the page as it stands and writes its findings and a screenshot.
export async function audit(page: Page, name: string): Promise<void> {
  await page.addScriptTag({ path: createRequire(import.meta.url).resolve("axe-core/axe.min.js") });
  const measured = await page.evaluate(measurePage);
  const viewport = page.viewportSize() ?? { width: 0, height: 0 };

  await mkdir(out, { recursive: true });
  await writeFile(`${out}${name}.png`, await page.screenshot({ animations: "disabled", caret: "hide" }));
  const screen: Screen = { name, viewport, ...measured };
  await writeFile(`${out}${name}.json`, JSON.stringify(screen));
}

/// Runs in the page.
async function measurePage(): Promise<{ findings: Finding[]; small: Record<string, number> }> {
  const findings: Finding[] = [];
  const small: Record<string, number> = {};
  const near = (d: number) => Math.abs(d) >= 0.5 && Math.abs(d) < 4;
  const r1 = (n: number) => Math.round(n * 10) / 10;
  const box = (r: DOMRect): Box => ({ x: r.left, y: r.top, w: r.width, h: r.height });

  // With a modal open, only what is on it can be seen.
  const modal = [...document.querySelectorAll("[aria-modal=true], dialog[open]")].filter(el => el.checkVisibility()).pop() ?? document.body;
  const shown = (el: Element) => {
    const r = el.getBoundingClientRect();
    return modal.contains(el) && r.width > 0 && r.height > 0 && r.right > 0 && r.bottom > 0 && r.left < innerWidth && r.top < innerHeight
      && el.checkVisibility({ opacityProperty: true, visibilityProperty: true });
  };
  const name = (el: Element) => el.tagName.toLowerCase() + [...el.classList].sort().map(c => `.${c}`).join("");
  const sig = (el: Element) => {
    const parent = el.parentElement?.closest("[class]");
    return `${parent ? `${name(parent)} > ` : ""}${name(el)}`;
  };
  const what = (el: Element) => (el.getAttribute("aria-label") ?? el.textContent ?? "").replace(/\s+/g, " ").trim().slice(0, 60);

  const alpha = (color: string) => {
    if (color === "transparent") return 0;
    const slash = color.match(/\/\s*([\d.]+)(%?)\s*\)$/);
    if (slash) return parseFloat(slash[1]) / (slash[2] ? 100 : 1);
    const rgba = color.match(/^rgba\([^,]+,[^,]+,[^,]+,\s*([\d.]+)\)$/);
    return rgba ? parseFloat(rgba[1]) : 1;
  };
  const painted = (el: Element) => {
    const s = getComputedStyle(el);
    const border = ["top", "right", "bottom", "left"].some(side => parseFloat(s.getPropertyValue(`border-${side}-width`)) > 0 && alpha(s.getPropertyValue(`border-${side}-color`)) > 0.03);
    return alpha(s.backgroundColor) > 0.03 || s.backgroundImage !== "none" || border || s.boxShadow !== "none";
  };
  /// The padding box: what a fill or border paints around the content.
  const inner = (el: Element): Box => {
    const r = el.getBoundingClientRect();
    const s = getComputedStyle(el);
    const top = parseFloat(s.borderTopWidth), left = parseFloat(s.borderLeftWidth);
    return { x: r.left + left, y: r.top + top, w: r.width - left - parseFloat(s.borderRightWidth), h: r.height - top - parseFloat(s.borderBottomWidth) };
  };

  // Font metrics, from a canvas set to the element's font.
  const canvas = document.createElement("canvas").getContext("2d");
  if (!canvas) throw new Error("no 2d canvas");
  const fonts = new Map<string, { ascent: number; cap: number }>();
  const font = (el: Element) => {
    const s = getComputedStyle(el);
    const key = `${s.fontStyle} ${s.fontWeight} ${s.fontSize} ${s.fontFamily}`;
    let metrics = fonts.get(key);
    if (!metrics) {
      canvas.font = key;
      const h = canvas.measureText("H");
      metrics = { ascent: h.fontBoundingBoxAscent, cap: h.actualBoundingBoxAscent };
      fonts.set(key, metrics);
    }
    return { key, ...metrics };
  };

  /// One run of text: its first line's ink left, cap top, cap middle and baseline.
  type Line = { el: Element; start: number; left: number; right: number; top: number; capTop: number; capMid: number; baseline: number; lines: number };
  const line = (node: Text): Line | null => {
    const el = node.parentElement;
    const text = node.textContent ?? "";
    const start = text.search(/\S/);
    if (!el || start < 0 || el.closest("svg") || !shown(el)) return null;

    const range = document.createRange();
    range.setStart(node, start);
    range.setEnd(node, text.trimEnd().length);
    const rects = [...range.getClientRects()].filter(r => r.width > 0);
    if (!rects.length) return null;

    const first = rects[0];
    const f = font(el);
    canvas.font = f.key;
    const bearing = canvas.measureText(text[start]).actualBoundingBoxLeft;
    const baseline = first.top + f.ascent;
    return { el, start: first.left, left: first.left - bearing, right: first.right, top: first.top, capTop: baseline - f.cap, capMid: baseline - f.cap / 2, baseline, lines: new Set(rects.map(r => Math.round(r.top))).size };
  };
  const lines = (root: Element) => {
    const found: Line[] = [];
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const l = node instanceof Text ? line(node) : null;
      if (l) found.push(l);
    }
    return found;
  };

  /// The bounds of an icon's drawn shapes, not its viewport.
  const ink = (svg: SVGSVGElement): Box => {
    let x1 = Infinity, y1 = Infinity, x2 = -Infinity, y2 = -Infinity;
    for (const shape of svg.querySelectorAll<SVGGraphicsElement>("path, circle, rect, line, polyline, polygon, ellipse")) {
      const b = shape.getBBox();
      const m = shape.getScreenCTM();
      if (!m || (b.width === 0 && b.height === 0)) continue;
      for (const [x, y] of [[b.x, b.y], [b.x + b.width, b.y], [b.x, b.y + b.height], [b.x + b.width, b.y + b.height]]) {
        const p = new DOMPoint(x, y).matrixTransform(m);
        x1 = Math.min(x1, p.x); y1 = Math.min(y1, p.y); x2 = Math.max(x2, p.x); y2 = Math.max(y2, p.y);
      }
    }
    return x1 === Infinity ? box(svg.getBoundingClientRect()) : { x: x1, y: y1, w: x2 - x1, h: y2 - y1 };
  };
  const icons = (root: Element) => [...root.querySelectorAll("svg")].filter(svg => shown(svg) && !svg.parentElement?.closest("svg"));

  const controls = [...document.querySelectorAll("button, a[href], summary, select, input:not([type=hidden]), [role=button], [role=tab], [role=option], [role=menuitem], [role=menuitemradio], [role=switch], [role=checkbox], [role=radio], [role=link]")].filter(shown);
  const iconOnly = new Set<Element>();

  // 1. An icon alone in a control sits in its middle.
  for (const control of controls) {
    const svgs = icons(control);
    if (svgs.length !== 1 || lines(control).length) continue;
    iconOnly.add(control);
    const b = inner(control), i = ink(svgs[0]);
    const left = i.x - b.x, right = b.x + b.w - i.x - i.w, top = i.y - b.y, bottom = b.y + b.h - i.y - i.h;
    const midY = i.y + i.h / 2, midX = i.x + i.w / 2;
    if (near(left - right)) findings.push({ check: "icon-centre-x", sig: sig(control), what: what(control), delta: r1(left - right), detail: `left ${r1(left)} / right ${r1(right)}`, box: b, marks: [{ x1: b.x, y1: midY, x2: i.x, y2: midY, label: `${r1(left)}` }, { x1: i.x + i.w, y1: midY, x2: b.x + b.w, y2: midY, label: `${r1(right)}` }] });
    if (near(top - bottom)) findings.push({ check: "icon-centre-y", sig: sig(control), what: what(control), delta: r1(top - bottom), detail: `top ${r1(top)} / bottom ${r1(bottom)}`, box: b, marks: [{ x1: midX, y1: b.y, x2: midX, y2: i.y, label: `${r1(top)}` }, { x1: midX, y1: i.y + i.h, x2: midX, y2: b.y + b.h, label: `${r1(bottom)}` }] });
  }

  // 2. A one-line label sits with its caps in the middle of a painted control.
  for (const control of controls) {
    if (iconOnly.has(control) || !painted(control)) continue;
    const ls = lines(control);
    const b = inner(control);
    if (!ls.length || b.h > 48 || ls.some(l => l.lines > 1 || Math.abs(l.baseline - ls[0].baseline) > 0.5)) continue;
    const l = ls[0];
    const top = l.capTop - b.y, bottom = b.y + b.h - l.baseline;
    const x = l.left + 2;
    if (near(top - bottom)) findings.push({ check: "label-centre", sig: sig(control), what: what(control), delta: r1(top - bottom), detail: `above caps ${r1(top)} / below baseline ${r1(bottom)}`, box: b, marks: [{ x1: x, y1: b.y, x2: x, y2: l.capTop, label: `${r1(top)}` }, { x1: x, y1: l.baseline, x2: x, y2: b.y + b.h, label: `${r1(bottom)}` }] });
  }

  // 3. An icon beside text is centred on the text's caps.
  const allLines = lines(document.body);
  for (const svg of icons(document.body)) {
    if (iconOnly.has(svg.parentElement?.closest("button, a, [role]") ?? svg)) continue;
    const i = ink(svg);
    const scope = svg.parentElement?.parentElement ?? document.body;
    const beside = allLines.filter(l => scope.contains(l.el) && l.top < i.y + i.h && l.top + (l.baseline - l.top) * 1.3 > i.y && Math.min(Math.abs(l.left - i.x - i.w), Math.abs(i.x - l.right)) < 16)
      .sort((a, b) => Math.abs(a.left - i.x - i.w) - Math.abs(b.left - i.x - i.w))[0];
    if (!beside) continue;
    const d = i.y + i.h / 2 - beside.capMid;
    const x = i.x + i.w + 1;
    if (near(d)) findings.push({ check: "icon-text", sig: `${sig(svg.parentElement ?? svg)} svg`, what: what(svg.parentElement?.parentElement ?? svg), delta: r1(d), detail: `icon middle ${d > 0 ? "below" : "above"} the caps' middle by ${r1(Math.abs(d))}`, box: { x: Math.min(i.x, beside.left), y: Math.min(i.y, beside.capTop), w: Math.max(i.x + i.w, beside.right) - Math.min(i.x, beside.left), h: Math.max(i.h, beside.baseline - beside.capTop) }, marks: [{ x1: i.x, y1: i.y + i.h / 2, x2: x, y2: i.y + i.h / 2, label: "" }, { x1: x, y1: beside.capMid, x2: beside.left + 6, y2: beside.capMid, label: `${r1(d)}` }] });
  }

  // 4. The first thing in a card is as far from its top as from its side,
  // when the two are nearly equal; so is an icon at its far right.
  for (const el of document.querySelectorAll("body *")) {
    if (iconOnly.has(el) || !shown(el) || !painted(el) || el.closest("svg")) continue;
    const b = inner(el);
    if (b.h < 20 || b.h > 240 || b.w > 900 || (controls.includes(el) && b.h < 36)) continue;
    const items = [
      ...lines(el).map(l => ({ x: l.left, y: l.capTop, r: l.right, h: l.baseline - l.capTop, svg: false })),
      ...icons(el).map(svg => ink(svg)).map(i => ({ x: i.x, y: i.y, r: i.x + i.w, h: i.h, svg: true })),
    ];
    if (!items.length) continue;
    const first = items.reduce((a, c) => c.y + c.x < a.y + a.x ? c : a);
    const left = first.x - b.x, top = first.y - b.y;
    if (near(left - top) && left < 30) findings.push({ check: "inset", sig: sig(el), what: what(el), delta: r1(left - top), detail: `first item: left ${r1(left)} / top ${r1(top)}`, box: b, marks: [{ x1: b.x, y1: first.y + first.h / 2, x2: first.x, y2: first.y + first.h / 2, label: `${r1(left)}` }, { x1: first.x + 2, y1: b.y, x2: first.x + 2, y2: first.y, label: `${r1(top)}` }] });
    const last = items.filter(c => c.svg && Math.abs(c.y + c.h / 2 - (first.y + first.h / 2)) < 6 && c !== first).sort((a, c) => c.r - a.r)[0];
    if (!last) continue;
    const right = b.x + b.w - last.r, lastTop = last.y - b.y;
    if (near(right - lastTop) && right < 30) findings.push({ check: "inset", sig: `${sig(el)} (right icon)`, what: what(el), delta: r1(right - lastTop), detail: `right icon: right ${r1(right)} / top ${r1(lastTop)}`, box: b, marks: [{ x1: last.r, y1: last.y + last.h / 2, x2: b.x + b.w, y2: last.y + last.h / 2, label: `${r1(right)}` }, { x1: last.r - 2, y1: b.y, x2: last.r - 2, y2: last.y, label: `${r1(lastTop)}` }] });
  }

  // 5. Left edges that nearly line up, within one region of the window.
  const regionOf = (el: Element) => el.closest("dialog, [role=dialog], [role=menu], [role=listbox], aside, nav, main, section, header") ?? document.body;
  const edges = [
    ...allLines.map(l => ({ el: l.el, x: l.start, y: l.top, svg: false })),
    ...icons(document.body).map(svg => ({ el: svg, x: ink(svg).x, y: ink(svg).y, svg: true })),
  ];
  const seen = new Set<string>();
  for (const a of edges) for (const b of edges) {
    if (a === b || a.svg !== b.svg || a.x > b.x || b.y <= a.y || b.y - a.y > 120 || regionOf(a.el) !== regionOf(b.el)) continue;
    const d = b.x - a.x;
    const key = `${sig(a.el)}|${sig(b.el)}`;
    if (d < 0.75 || d >= 3 || seen.has(key)) continue;
    seen.add(key);
    const top = Math.min(a.y, b.y) - 4, bottom = Math.max(a.y, b.y) + 16;
    findings.push({ check: "edge", sig: key, what: `${what(a.el).slice(0, 28)} / ${what(b.el).slice(0, 28)}`, delta: r1(d), detail: `left edges ${r1(a.x)} and ${r1(b.x)}`, box: { x: a.x - 8, y: top, w: d + 60, h: bottom - top }, marks: [{ x1: a.x, y1: top, x2: a.x, y2: bottom, label: "" }, { x1: b.x, y1: top, x2: b.x, y2: bottom, label: `${r1(d)}` }] });
  }

  // 6. Same siblings, nearly the same height.
  for (const parent of document.querySelectorAll("body *")) {
    const kids = [...parent.children].filter(shown);
    const groups = new Map<string, Element[]>();
    for (const kid of kids) groups.set(name(kid), [...(groups.get(name(kid)) ?? []), kid]);
    for (const [, group] of groups) {
      if (group.length < 2 || !group.every(painted)) continue;
      const heights = group.map(el => el.getBoundingClientRect().height);
      const lo = Math.min(...heights), hi = Math.max(...heights);
      const short = group[heights.indexOf(lo)], tall = group[heights.indexOf(hi)];
      if (near(hi - lo) && lines(short).length === lines(tall).length) findings.push({ check: "sibling-height", sig: sig(tall), what: `${what(short).slice(0, 28)} / ${what(tall).slice(0, 28)}`, delta: r1(hi - lo), detail: `heights ${r1(lo)} and ${r1(hi)}`, box: box(tall.getBoundingClientRect()), marks: [] });
    }
  }

  // 7. Text cut off with no ellipsis.
  for (const el of document.querySelectorAll("body *")) {
    if (!shown(el) || el.closest("svg")) continue;
    const s = getComputedStyle(el);
    const hasText = [...el.childNodes].some(n => n.nodeType === Node.TEXT_NODE && n.textContent?.trim());
    if (!hasText || !["hidden", "clip"].includes(s.overflowX) || s.textOverflow === "ellipsis" || el.scrollWidth <= el.clientWidth + 1) continue;
    findings.push({ check: "clipped", sig: sig(el), what: what(el), delta: el.scrollWidth - el.clientWidth, detail: `${el.scrollWidth - el.clientWidth}px of text hidden`, box: box(el.getBoundingClientRect()), marks: [] });
  }

  // Targets under 24px, counted.
  for (const control of controls) {
    const r = control.getBoundingClientRect();
    if (r.width < 24 || r.height < 24) small[sig(control)] = (small[sig(control)] ?? 0) + 1;
  }

  // axe-core.
  const result = await window.axe.run(document, { runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"] }, resultTypes: ["violations"] });
  for (const violation of result.violations) for (const node of violation.nodes) {
    const target = node.target[0];
    const el = typeof target === "string" ? document.querySelector(target) : null;
    if (!el) continue;
    const summary = (node.failureSummary ?? "").split("\n").slice(1).join(" ").trim();
    findings.push({ check: `axe ${violation.id}`, sig: sig(el), what: what(el), delta: 0, detail: `${violation.help}. ${summary}`.slice(0, 240), box: box(el.getBoundingClientRect()), marks: [] });
  }

  return { findings, small };
}

// Report

const checks: Record<string, string> = {
  "inset": "Inset: first item nearly as far from the top as from the side",
  "icon-centre-x": "Icon-only control: icon off centre, horizontally",
  "icon-centre-y": "Icon-only control: icon off centre, vertically",
  "icon-text": "Icon beside text: off the caps' middle",
  "label-centre": "Label in a control: caps off centre",
  "edge": "Left edges that nearly line up",
  "sibling-height": "Same siblings, nearly the same height",
  "clipped": "Text cut off with no ellipsis",
};

type Group = { check: string; finding: Finding; screen: Screen; screens: string[] };

async function report(): Promise<void> {
  const screens: Screen[] = [];
  for (const file of (await readdir(out)).filter(f => f.endsWith(".json")).sort()) screens.push(JSON.parse(await readFile(`${out}${file}`, "utf8")));

  const groups = new Map<string, Group>();
  for (const screen of screens) for (const finding of screen.findings) {
    const key = `${finding.check}|${finding.sig}|${Math.round(finding.delta * 2) / 2}`;
    const group = groups.get(key);
    if (group) { if (!group.screens.includes(screen.name)) group.screens.push(screen.name); }
    else groups.set(key, { check: finding.check, finding, screen, screens: [screen.name] });
  }

  const order = [...Object.keys(checks), ...[...new Set([...groups.values()].map(g => g.check))].filter(c => !(c in checks)).sort()];
  const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
  const crop = ({ finding: f, screen }: Group) => {
    const pad = 14;
    const x = f.box.x - pad, y = f.box.y - pad, w = f.box.w + pad * 2, h = f.box.h + pad * 2;
    const scale = Math.min(4, 520 / w, 260 / h);
    const size = Math.max(3, 11 / scale);
    const marks = f.marks.map(m => `<line x1="${m.x1}" y1="${m.y1}" x2="${m.x2}" y2="${m.y2}" stroke="#f33" stroke-width="${1 / scale * 1.5}"/>${m.label ? `<text x="${(m.x1 + m.x2) / 2 + size * 0.3}" y="${(m.y1 + m.y2) / 2 + size * 0.35}" font-size="${size}" fill="#fff" stroke="#c00" stroke-width="${size / 5}" paint-order="stroke">${m.label}</text>` : ""}`).join("");
    return `<svg width="${w * scale}" height="${h * scale}" viewBox="${x} ${y} ${w} ${h}"><image href="${screen.name}.png" width="${screen.viewport.width}" height="${screen.viewport.height}" style="image-rendering:pixelated"/><rect x="${f.box.x}" y="${f.box.y}" width="${f.box.w}" height="${f.box.h}" fill="none" stroke="#3af" stroke-dasharray="${2 / scale} ${2 / scale}" stroke-width="${1 / scale}"/>${marks}</svg>`;
  };

  const sections = order.map(check => {
    const list = [...groups.values()].filter(g => g.check === check).sort((a, b) => b.screens.length - a.screens.length || Math.abs(b.finding.delta) - Math.abs(a.finding.delta));
    if (!list.length) return "";
    const items = list.map(g => `<article><div class="meta"><b>${esc(g.finding.what || "(no text)")}</b><code>${esc(g.finding.sig)}</code><span>${esc(g.finding.detail)}</span><span class="dim">${g.screens.length} screen${g.screens.length > 1 ? "s" : ""}: ${esc(g.screens.slice(0, 6).join(", "))}${g.screens.length > 6 ? ", ..." : ""}</span></div>${crop(g)}</article>`).join("");
    return `<section id="${esc(check)}"><h2>${esc(checks[check] ?? check)} <small>${list.length}</small></h2>${items}</section>`;
  }).join("");

  const small = new Map<string, number>();
  for (const screen of screens) for (const [s, n] of Object.entries(screen.small)) small.set(s, (small.get(s) ?? 0) + n);
  const smallList = [...small].sort((a, b) => b[1] - a[1]).slice(0, 20).map(([s, n]) => `<li><code>${esc(s)}</code> ${n}</li>`).join("");
  const toc = order.map(check => [check, [...groups.values()].filter(g => g.check === check).length] as const).filter(([, n]) => n).map(([check, n]) => `<li><a href="#${esc(check)}">${esc(checks[check] ?? check)}</a> ${n}</li>`).join("");

  await writeFile(`${out}report.html`, `<!doctype html><meta charset="utf-8"><title>UI audit</title><style>
body{font:13px/1.5 ui-sans-serif,system-ui;background:#111;color:#ddd;margin:24px;max-width:1200px}
code{font:11px ui-monospace,monospace;color:#9ab;word-break:break-all}.dim{color:#888}
article{display:flex;gap:16px;align-items:flex-start;padding:10px 0;border-top:1px solid #222}
.meta{flex:1;display:flex;flex-direction:column;gap:2px;min-width:0}svg{flex:none;background:#000}
h2 small{color:#888;font-weight:normal}a{color:#8bf}
</style><h1>UI audit</h1><p>${screens.length} screens, ${[...groups.values()].length} distinct findings. Chromium at 2x; the app on macOS, iOS and Linux is WebKit, so check a finding there before fixing it. Differences of 0.5 to 4px count; bigger ones are taken as intended. Transitions are not measured: the baseline runs with reduced motion.</p><ul>${toc}</ul>${sections}<h2>Targets under 24px <small>by control, summed over screens</small></h2><ul>${smallList}</ul>`);
  console.log(`${out}report.html: ${groups.size} findings over ${screens.length} screens`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) await report();
