import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { chartSizer, chartValue, CHART_COLORS, CHART_RANGES, drawPlot, type Chart, type ChartCard, type Plot } from "./charts.ts";
import { invoke } from "../ui/tauri.ts";

export function ChartView({ card, initial }: { card: ChartCard; initial: Chart }) {
  const [chart, setChart] = useState(initial), [off, setOff] = useState<Set<number>>(() => new Set());
  const [error, setError] = useState(""), [pending, setPending] = useState<string | null>(null);
  const plot = useRef<HTMLDivElement>(null), generation = useRef(0);
  useEffect(() => () => { generation.current++; }, []);
  useLayoutEffect(() => {
    const el = plot.current as Plot | null;
    if (!el) return;
    card.chart = chart; card.off = off; el.chart = chart; el.card = card;
    chartSizer.observe(el); drawPlot(el);
    return () => { chartSizer.unobserve(el); };
  }, [card, chart, off]);
  const stock = chart.stock, many = chart.series.length > 1;
  const toggle = (index: number) => setOff(previous => {
    const next = new Set(previous);
    if (next.has(index)) next.delete(index); else if (next.size < chart.series.length - 1) next.add(index);
    return next;
  });
  const chooseRange = async (range: string) => {
    if (!stock || range === (pending ?? stock.range)) return;
    const mine = ++generation.current;
    setPending(range); setError("");
    try {
      const result = await invoke<Chart>("stock_chart", { symbol: stock.symbol, range });
      if (generation.current === mine) { setChart(result); setOff(new Set()); setPending(null); }
    } catch (err) {
      if (generation.current === mine) { setPending(null); setError(String(err)); }
    }
  };
  const change = stock ? stock.last - stock.previous : 0, pct = stock?.previous ? change / stock.previous * 100 : 0;
  const selectedRange = pending ?? stock?.range;
  return <><div className="ch"><span className="tt"><b>{stock && !many ? stock.symbol : chart.title}</b>{stock ? <small>{many ? "Change over the range, in %" : [stock.name, stock.exchange, stock.currency].filter(Boolean).join(" · ")}</small> : chart.unit && <small>{chart.unit}</small>}</span>{stock && !many && <span className="px"><b>{chartValue({ unit: stock.currency }, stock.last)}</b><small className={change >= 0 ? "up" : "down"}>{chartValue({ unit: stock.currency }, change, true)} ({pct > 0 ? "+" : ""}{pct.toFixed(2)}%) {CHART_RANGES.find(([range]) => range === stock.range)?.[1] ?? ""}</small></span>}</div>
    <div ref={plot} className={`plot${pending ? " wait" : ""}`} role="img" aria-label={chart.title} /><div className="derr">{error}</div>
    {(stock && !many || many || chart.source) && <div className="foot">{stock && !many && <span className="ranges">{CHART_RANGES.map(([range, label]) => <button type="button" key={range} data-range={range} aria-pressed={range === selectedRange} onClick={event => { event.stopPropagation(); void chooseRange(range); }}>{label}</button>)}</span>}{many && <span className="legend">{chart.series.map((series, index) => <button type="button" key={index} data-series={index} aria-pressed={!off.has(index)} onClick={event => { event.stopPropagation(); toggle(index); }}><i style={{ background: CHART_COLORS[index] }} />{series.name}</button>)}</span>}{chart.source && <span className="src">{chart.source}</span>}</div>}</>;
}
