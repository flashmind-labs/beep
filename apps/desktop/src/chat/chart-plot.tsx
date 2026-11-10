import { Fragment, memo, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type MouseEvent } from "react";
import { CHART_COLORS, CHART_H, chartTime, chartValue, nearest, niceTicks, plotScale, shownSeries, type Chart } from "./charts.ts";

export function ChartPlot({ chart, off, waiting }: { chart: Chart; off: ReadonlySet<number>; waiting: boolean }) {
  const host = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(600), [hover, setHover] = useState<number | null>(null);
  const frame = useRef<number | null>(null), position = useRef<number | null>(null);
  const gradient = useId().replace(/:/g, "");
  useLayoutEffect(() => {
    const node = host.current;
    if (!node) return;
    const measure = () => setWidth(node.clientWidth || 600);
    const observer = new ResizeObserver(measure);
    measure(); observer.observe(node);
    return () => observer.disconnect();
  }, []);
  useEffect(() => () => { if (frame.current !== null) cancelAnimationFrame(frame.current); }, []);
  const geometry = useMemo(() => plotScale(chart, off, width), [chart, off, width]);
  const shown = useMemo(() => shownSeries(chart, off), [chart, off]);
  const move = (event: MouseEvent<HTMLDivElement>) => {
    position.current = event.clientX - event.currentTarget.getBoundingClientRect().left;
    if (frame.current !== null) return;
    frame.current = requestAnimationFrame(() => { frame.current = null; setHover(position.current); });
  };
  const leave = () => { position.current = null; if (frame.current !== null) cancelAnimationFrame(frame.current); frame.current = null; setHover(null); };
  return <div ref={host} className={`plot${waiting ? " wait" : ""}`} role="img" aria-label={chart.title} onMouseMove={move} onMouseLeave={leave}>
    <svg width={geometry.W} height={CHART_H} viewBox={`0 0 ${geometry.W} ${CHART_H}`}>
      <PlotGeometry chart={chart} shown={shown} geometry={geometry} gradient={gradient} />
      <HoverMarks chart={chart} shown={shown} geometry={geometry} hover={hover} />
    </svg>
    <Tooltip chart={chart} shown={shown} geometry={geometry} hover={hover} />
  </div>;
}
type Geometry = ReturnType<typeof plotScale>;
type Shown = ReturnType<typeof shownSeries>;
type PlotProps = { chart: Chart; shown: Shown; geometry: Geometry };
const PlotGeometry = memo(function PlotGeometry({ chart: c, shown, geometry: g, gradient }: PlotProps & { gradient: string }) {
  const { ticks, digits } = niceTicks(g.lo, g.hi, 3);
  const every = Math.ceil(c.categories.length / Math.max(1, Math.floor(g.plotW / 70)));
  const count = Math.max(2, Math.min(5, Math.floor(g.plotW / 110)));
  const base = c.stock && c.series.length === 1 && c.kind !== "bar" ? c.stock.previous : c.unit === "%" ? 0 : null;
  return <>
    {ticks.map(v => <Fragment key={v}><line x1={0} x2={g.plotW} y1={g.Y(v)} y2={g.Y(v)} className="gl" /><text x={g.plotW + 6} y={g.Y(v) + 4} className="ax">{chartValue(c, v, false, digits)}</text></Fragment>)}
    {base !== null && <line x1={0} x2={g.plotW} y1={g.Y(base)} y2={g.Y(base)} className="base" />}
    {shown.map(({ s, i }, k) => {
      if (c.kind === "bar") {
        const slot = g.plotW / c.categories.length, bw = slot * .7 / Math.max(1, shown.length);
        return <Fragment key={i}>{s.points.map((p, j) => <rect key={j} x={g.X(p.x) - slot * .35 + k * bw} y={g.Y(Math.max(p.y, 0))} width={Math.max(1, bw - 2)} height={Math.max(1, g.Y(Math.min(p.y, 0)) - g.Y(Math.max(p.y, 0)))} rx={2.5} fill={CHART_COLORS[i]} />)}</Fragment>;
      }
      const d = s.points.map((p, j) => `${j ? "L" : "M"}${g.X(p.x).toFixed(1)},${g.Y(p.y).toFixed(1)}`).join("");
      const first = s.points[0], last = s.points.at(-1), id = `${gradient}-${i}`;
      return <Fragment key={i}>{c.kind === "area" && shown.length === 1 && first && last && <><defs><linearGradient id={id} x1="0" x2="0" y1="0" y2="1"><stop offset="0" stopColor={CHART_COLORS[i]} stopOpacity=".35" /><stop offset="1" stopColor={CHART_COLORS[i]} stopOpacity="0" /></linearGradient></defs><path d={`${d}L${g.X(last.x)},${g.plotH}L${g.X(first.x)},${g.plotH}Z`} fill={`url(#${id})`} /></>}<path d={d} fill="none" stroke={CHART_COLORS[i]} strokeWidth="1.8" strokeLinejoin="round" strokeLinecap="round" /></Fragment>;
    })}
    {c.x === "category" ? c.categories.map((label, i) => i % every ? null : <text key={i} x={g.X(i)} y={CHART_H - 4} className="ax" textAnchor="middle">{label}</text>) : Array.from({ length: count }, (_, i) => { const x = g.x0 + g.span * i / (count - 1); return <text key={i} x={g.X(x)} y={CHART_H - 4} className="ax" textAnchor={i === 0 ? "start" : i === count - 1 ? "end" : "middle"}>{chartTime(x, g.span)}</text>; })}
  </>;
});
function hitsAt({ chart: c, shown, geometry: g, hover }: PlotProps & { hover: number | null }) {
  if (hover === null || hover > g.plotW + 4) return [];
  const x = c.kind === "bar" ? Math.round(g.x0 + hover / g.plotW * g.span) : g.x0 + Math.max(0, Math.min(1, hover / g.plotW)) * g.span;
  return shown.filter(({ s }) => s.points.length).map(({ s, i }) => ({ s, i, p: nearest(s.points, x) }));
}
function HoverMarks(p: PlotProps & { hover: number | null }) {
  const hits = hitsAt(p), g = p.geometry;
  return <g className="hov">{p.chart.kind !== "bar" && hits.length > 0 && <><line x1={g.X(hits[0].p.x)} x2={g.X(hits[0].p.x)} y1={0} y2={g.plotH} className="cross" />{hits.map(({ i, p: point }) => <circle key={i} cx={g.X(point.x)} cy={g.Y(point.y)} r="3.5" fill={CHART_COLORS[i]} stroke="#121916" strokeWidth="2" />)}</>}</g>;
}
function Tooltip(p: PlotProps & { hover: number | null }) {
  const ref = useRef<HTMLDivElement>(null), hits = hitsAt(p), g = p.geometry;
  useLayoutEffect(() => {
    const tip = ref.current;
    if (!tip || !hits.length) return;
    const half = tip.offsetWidth / 2;
    tip.style.left = `${Math.max(half, Math.min(g.plotW - half, g.X(hits[0].p.x)))}px`;
  });
  const at = hits[0]?.p.x;
  const label = at === undefined ? "" : p.chart.x === "time" ? chartTime(at, g.span, true) : p.chart.categories[Math.round(at)] ?? "";
  return <div ref={ref} className="tipc" hidden={!hits.length}>{label} <span>·</span> {hits.map(({ s, i, p: point }, index) => <Fragment key={i}>{index > 0 && <> <span>·</span> </>}{hits.length > 1 ? `${s.name} ${chartValue(p.chart, point.y, p.chart.unit === "%")}` : chartValue(p.chart, point.y)}</Fragment>)}</div>;
}
