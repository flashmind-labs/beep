import { esc, h, invoke, type Turn } from "./core.ts";
import { sentBox } from "./media.ts";

// ---------- Charts ----------

export interface ChartPoint {
  x: number;
  y: number;
}

/// The quote a stock chart is drawn for (StockInfo).
export interface StockInfo {
  symbol: string;
  name: string;
  exchange: string;
  currency: string;
  range: string;
  last: number;
  previous: number;
}

/// A chart under a reply (Chart), from a tool result or `stock_chart`.
export interface Chart {
  title: string;
  kind: "line" | "area" | "bar";
  /// A time in Unix milliseconds, or an index into `categories`.
  x: "time" | "category";
  categories: string[];
  /// Such as "USD" or "%"; empty when unknown.
  unit: string;
  series: { name: string; points: ChartPoint[] }[];
  stock: StockInfo | null;
  source: string;
}

/// A chart card: the chart and the series switched off in its legend.
interface ChartCard extends HTMLDivElement {
  chart: Chart;
  off: Set<number>;
}

/// A card's plot, which knows its chart and the width it was drawn at.
interface Plot extends HTMLDivElement {
  chart: Chart;
  card: ChartCard;
  drawnWidth?: number;
}

/// show_chart and show_stock_chart results. The plot is drawn at the card's
/// own width, so text never stretches, and again when the bar resizes.
export const CHART_COLORS = ["#3B9B7A", "#F07A6A", "#8FB3E8", "#E8C46A"];
export const CHART_RANGES = [["1d", "1D"], ["5d", "5D"], ["1mo", "1M"], ["6mo", "6M"], ["ytd", "YTD"], ["1y", "1Y"], ["5y", "5Y"], ["max", "Max"]];
export const CHART_H = 150, CHART_PAD = { top: 8, right: 52, bottom: 20 };
export const chartSizer = new ResizeObserver(entries => {
  for (const { target } of entries) {
    const plot = target as Plot;
    if (plot.chart && plot.clientWidth !== plot.drawnWidth) drawPlot(plot);
  }
});

export function chartRow(turn: Turn, chart: Chart) {
  const card = h<ChartCard>('<div class="dchart"></div>');
  card.chart = chart;
  card.off = new Set();
  sentBox(turn).append(card);
  drawChart(card);
}

/// A value in the chart's unit: a currency code as money, % with its sign,
/// anything else after the number.
/// `digits` fixes the decimals, as an axis needs; otherwise they follow the size.
export function chartValue(chart: { unit: string }, v: number, signed = false, digits: number | null = null) {
  const unit = chart.unit;
  const sign = signed && v > 0 ? "+" : "";
  const most = digits ?? (Math.abs(v) < 1000 ? 2 : 0);
  const fixed = { minimumFractionDigits: digits ?? 0, maximumFractionDigits: most };
  if (unit === "%") return sign + new Intl.NumberFormat(undefined, fixed).format(v) + "%";
  if (/^[A-Z]{3}$/.test(unit)) {
    try {
      return sign + new Intl.NumberFormat(undefined, { style: "currency", currency: unit, ...fixed, ...(digits == null && Math.abs(v) < 1000 ? { minimumFractionDigits: 2 } : {}) }).format(v);
    } catch {}
  }
  return sign + new Intl.NumberFormat(undefined, fixed).format(v) + (unit ? " " + unit : "");
}
/// Round values between `lo` and `hi` for the value axis, about `n` apart.
export function niceTicks(lo: number, hi: number, n: number) {
  const raw = (hi - lo) / n, mag = 10 ** Math.floor(Math.log10(raw || 1));
  const step = [1, 2, 2.5, 5, 10].map(m => m * mag).find(st => st >= raw) ?? raw;
  const ticks = [];
  for (let v = Math.ceil(lo / step) * step; v <= hi + step * 1e-9; v += step) ticks.push(Math.abs(v) < step * 1e-9 ? 0 : v);
  return { ticks, digits: Math.max(0, -Math.floor(Math.log10(step) + 1e-9)) };
}
/// An axis label for time `ms`, as fine as the chart's span needs.
export function chartTime(ms: number, span: number, full = false) {
  const d = new Date(ms);
  if (span < 2 * 86400e3) return d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" }) + (full ? ", " + d.toLocaleDateString(undefined, { day: "numeric", month: "short" }) : "");
  if (span < 400 * 86400e3) return d.toLocaleDateString(undefined, { day: "numeric", month: "short", ...(full ? { year: "numeric" } : {}) });
  return d.toLocaleDateString(undefined, { month: "short", year: "numeric", ...(full ? { day: "numeric" } : {}) });
}
export function xLabel(chart: Chart, x: number, span: number, full: boolean) {
  return chart.x === "time" ? chartTime(x, span, full) : chart.categories[Math.round(x)] ?? "";
}

export function drawChart(card: ChartCard) {
  const c = card.chart, st = c.stock, many = c.series.length > 1;
  let head;
  if (st) {
    const change = st.last - st.previous, pct = st.previous ? change / st.previous * 100 : 0;
    const range = CHART_RANGES.find(([r]) => r === st.range)?.[1] ?? "";
    const meta = [st.name, st.exchange, st.currency].filter(Boolean).map(esc).join(" · ");
    const money = { unit: st.currency };
    head = `<div class="ch"><span class="tt"><b>${esc(many ? c.title : st.symbol)}</b><small>${many ? esc("Change over the range, in %") : meta}</small></span>
      ${many ? "" : `<span class="px"><b>${esc(chartValue(money, st.last))}</b><small class="${change >= 0 ? "up" : "down"}">${esc(chartValue(money, change, true))} (${pct > 0 ? "+" : ""}${pct.toFixed(2)}%) ${range}</small></span>`}</div>`;
  } else {
    head = `<div class="ch"><span class="tt"><b>${esc(c.title)}</b>${c.unit ? `<small>${esc(c.unit)}</small>` : ""}</span></div>`;
  }
  const legend = !many ? ""
    : `<span class="legend">${c.series.map((s, i) => `<button data-series="${i}" aria-pressed="${!card.off.has(i)}"><i style="background:${CHART_COLORS[i]}"></i>${esc(s.name)}</button>`).join("")}</span>`;
  // A comparison has no range buttons: StockChart answers one symbol.
  const ranges = st && c.series.length === 1 ? `<span class="ranges">${CHART_RANGES.map(([r, l]) => `<button data-range="${r}" aria-pressed="${r === st.range}">${l}</button>`).join("")}</span>` : "";
  const source = c.source ? `<span class="src">${esc(c.source)}</span>` : "";
  card.innerHTML = `${head}<div class="plot" role="img" aria-label="${esc(c.title)}"></div><div class="derr"></div>${ranges || legend || source ? `<div class="foot">${ranges}${legend}${source}</div>` : ""}`;
  const plot = card.querySelector<Plot>(".plot")!;
  plot.chart = c;
  plot.card = card;
  chartSizer.observe(plot);
  drawPlot(plot);
}

/// The series the legend leaves on, with their index for the colour.
const shownSeries = (plot: Plot) => plot.chart.series.map((s, i) => ({ s, i })).filter(({ i }) => !plot.card.off.has(i));

/// The plot's geometry: what each axis spans and where a value lands.
export function plotScale(plot: Plot) {
  const c = plot.chart, W = plot.clientWidth || 600;
  const shown = shownSeries(plot);
  const xs = c.series.flatMap(s => s.points.map(p => p.x)), ys = shown.flatMap(({ s }) => s.points.map(p => p.y));
  const bar = c.kind === "bar";
  let lo = Math.min(...ys), hi = Math.max(...ys);
  if (bar) { lo = Math.min(0, lo); hi = Math.max(0, hi); }
  if (c.stock && !bar && shown.length === 1 && c.series.length === 1) { lo = Math.min(lo, c.stock.previous); hi = Math.max(hi, c.stock.previous); }
  if (!isFinite(lo)) { lo = 0; hi = 1; }
  const pad = (hi - lo) * .06 || Math.abs(hi) * .05 || 1;
  if (!bar) { lo -= pad; hi += pad; } else hi += pad;
  const x0 = bar ? -.5 : Math.min(...xs), x1 = bar ? c.categories.length - .5 : Math.max(...xs);
  const plotW = W - CHART_PAD.right, plotH = CHART_H - CHART_PAD.bottom;
  return {
    W, plotW, plotH, lo, hi, x0, x1, span: x1 - x0,
    X: (x: number) => (x1 === x0 ? plotW / 2 : (x - x0) / (x1 - x0) * plotW),
    Y: (y: number) => CHART_PAD.top + (1 - (y - lo) / (hi - lo)) * (plotH - CHART_PAD.top),
  };
}

export function drawPlot(plot: Plot) {
  const c = plot.chart, g = plotScale(plot);
  plot.drawnWidth = plot.clientWidth;
  const { ticks, digits } = niceTicks(g.lo, g.hi, 3);
  const grid = ticks.map(v => `<line x1="0" x2="${g.plotW}" y1="${g.Y(v)}" y2="${g.Y(v)}" class="gl"/><text x="${g.plotW + 6}" y="${g.Y(v) + 4}" class="ax">${esc(chartValue(c, v, false, digits))}</text>`).join("");
  let xticks;
  if (c.x === "category") {
    const every = Math.ceil(c.categories.length / Math.max(1, Math.floor(g.plotW / 70)));
    xticks = c.categories.map((l, i) => i % every ? "" : `<text x="${g.X(i)}" y="${CHART_H - 4}" class="ax" text-anchor="middle">${esc(l)}</text>`).join("");
  } else {
    const n = Math.max(2, Math.min(5, Math.floor(g.plotW / 110)));
    xticks = Array.from({ length: n }, (_, i) => {
      const x = g.x0 + g.span * i / (n - 1);
      return `<text x="${g.X(x)}" y="${CHART_H - 4}" class="ax" text-anchor="${i === 0 ? "start" : i === n - 1 ? "end" : "middle"}">${esc(chartTime(x, g.span))}</text>`;
    }).join("");
  }
  let marks;
  const shown = shownSeries(plot);
  if (c.kind === "bar") {
    const slot = g.plotW / c.categories.length, bw = slot * .7 / Math.max(1, shown.length);
    marks = shown.map(({ s, i }, k) => s.points.map(p => {
      const x = g.X(p.x) - slot * .35 + k * bw, y = g.Y(Math.max(p.y, 0)), y0 = g.Y(Math.min(p.y, 0));
      return `<rect x="${x}" y="${y}" width="${Math.max(1, bw - 2)}" height="${Math.max(1, y0 - y)}" rx="2.5" fill="${CHART_COLORS[i]}"/>`;
    }).join("")).join("");
  } else {
    marks = shown.map(({ s, i }) => {
      const d = s.points.map((p, j) => `${j ? "L" : "M"}${g.X(p.x).toFixed(1)},${g.Y(p.y).toFixed(1)}`).join("");
      const area = c.kind === "area" && shown.length === 1
        ? `<defs><linearGradient id="cg${i}" x1="0" x2="0" y1="0" y2="1"><stop offset="0" stop-color="${CHART_COLORS[i]}" stop-opacity=".35"/><stop offset="1" stop-color="${CHART_COLORS[i]}" stop-opacity="0"/></linearGradient></defs><path d="${d}L${g.X(s.points.at(-1)!.x)},${g.plotH}L${g.X(s.points[0].x)},${g.plotH}Z" fill="url(#cg${i})"/>`
        : "";
      return `${area}<path d="${d}" fill="none" stroke="${CHART_COLORS[i]}" stroke-width="1.8" stroke-linejoin="round" stroke-linecap="round"/>`;
    }).join("");
  }
  const baseAt = c.stock && c.series.length === 1 && c.kind !== "bar" ? c.stock.previous : c.unit === "%" ? 0 : null;
  const base = baseAt == null ? "" : `<line x1="0" x2="${g.plotW}" y1="${g.Y(baseAt)}" y2="${g.Y(baseAt)}" class="base"/>`;
  plot.innerHTML = `<svg width="${g.W}" height="${CHART_H}" viewBox="0 0 ${g.W} ${CHART_H}">${grid}${base}${marks}${xticks}<g class="hov"></g></svg><div class="tipc" hidden></div>`;
  plot.onmousemove = e => hoverPlot(plot, e);
  plot.onmouseleave = () => { plot.querySelector(".hov")!.innerHTML = ""; plot.querySelector<HTMLElement>(".tipc")!.hidden = true; };
}

/// The point of `points` nearest to `x`, by binary search on sorted x.
export function nearest(points: ChartPoint[], x: number) {
  let a = 0, b = points.length - 1;
  while (b - a > 1) { const m = (a + b) >> 1; points[m].x < x ? a = m : b = m; }
  return Math.abs(points[a].x - x) <= Math.abs(points[b].x - x) ? points[a] : points[b];
}
export function hoverPlot(plot: Plot, e: MouseEvent) {
  const c = plot.chart, g = plotScale(plot);
  const px = e.clientX - plot.getBoundingClientRect().left;
  if (px > g.plotW + 4) return plot.onmouseleave!(e);
  const x = c.kind === "bar" ? Math.round(g.x0 + px / g.plotW * g.span) : g.x0 + Math.max(0, Math.min(1, px / g.plotW)) * g.span;
  const hits = shownSeries(plot).filter(({ s }) => s.points.length).map(({ s, i }) => ({ s, i, p: nearest(s.points, x) }));
  if (!hits.length) return;
  const at = hits[0].p.x, hx = g.X(at);
  plot.querySelector(".hov")!.innerHTML = c.kind === "bar" ? ""
    : `<line x1="${hx}" x2="${hx}" y1="0" y2="${g.plotH}" class="cross"/>` + hits.map(({ i, p }) => `<circle cx="${g.X(p.x)}" cy="${g.Y(p.y)}" r="3.5" fill="${CHART_COLORS[i]}" stroke="#121916" stroke-width="2"/>`).join("");
  const tip = plot.querySelector<HTMLElement>(".tipc")!;
  const values = hits.map(({ s, p }) => hits.length > 1 ? `${esc(s.name)} ${esc(chartValue(c, p.y, c.unit === "%"))}` : esc(chartValue(c, p.y))).join(" <span>·</span> ");
  tip.innerHTML = `${esc(xLabel(c, at, g.span, true))} <span>·</span> ${values}`;
  tip.hidden = false;
  const half = tip.offsetWidth / 2;
  tip.style.left = Math.max(half, Math.min(g.plotW - half, hx)) + "px";
}

export async function chartClick(e: MouseEvent) {
  const t = e.target as HTMLElement;
  const card = t.closest<ChartCard>(".dchart");
  if (!card) return false;
  const series = t.closest<HTMLElement>("[data-series]");
  if (series) {
    const i = +series.dataset.series!;
    if (card.off.has(i)) card.off.delete(i);
    else if (card.off.size < card.chart.series.length - 1) card.off.add(i);
    drawChart(card);
    return true;
  }
  const range = t.closest<HTMLElement>("[data-range]");
  if (!range || !card.chart.stock || range.getAttribute("aria-pressed") === "true") return !!range;
  const plot = card.querySelector(".plot")!;
  plot.classList.add("wait");
  card.querySelectorAll("[data-range]").forEach(b => b.setAttribute("aria-pressed", String(b === range)));
  try {
    card.chart = await invoke<Chart>("stock_chart", { symbol: card.chart.stock.symbol, range: range.dataset.range });
    card.off.clear();
    drawChart(card);
  } catch (err) {
    plot.classList.remove("wait");
    card.querySelector(".derr")!.textContent = String(err);
  }
  return true;
}
