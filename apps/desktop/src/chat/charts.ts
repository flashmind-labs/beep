
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

/// Chart dimensions and colors shared with the React plot.
export const CHART_COLORS = ["#3B9B7A", "#F07A6A", "#8FB3E8", "#E8C46A"];
export const CHART_RANGES = [["1d", "1D"], ["5d", "5D"], ["1mo", "1M"], ["6mo", "6M"], ["ytd", "YTD"], ["1y", "1Y"], ["5y", "5Y"], ["max", "Max"]];
export const CHART_H = 150, CHART_PAD = { top: 8, right: 52, bottom: 20 };

/// A value in the chart's unit: a currency code as money, % with its sign,
/// anything else after the number.
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

export const shownSeries = (chart: Chart, off: ReadonlySet<number>) => chart.series.map((s, i) => ({ s, i })).filter(({ i }) => !off.has(i));

/// The plot's geometry: what each axis spans and where a value lands.
export function plotScale(c: Chart, off: ReadonlySet<number>, width: number) {
  const W = width || 600;
  const shown = shownSeries(c, off);
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

/// The point of `points` nearest to `x`, by binary search on sorted x.
export function nearest(points: ChartPoint[], x: number) {
  let a = 0, b = points.length - 1;
  while (b - a > 1) { const m = (a + b) >> 1; points[m].x < x ? a = m : b = m; }
  return Math.abs(points[a].x - x) <= Math.abs(points[b].x - x) ? points[a] : points[b];
}
