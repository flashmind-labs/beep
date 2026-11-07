// ---- Desktop: charts ----
// show_stock_chart and show_chart results draw under the reply. Only the macOS app gets these tools.
function dSeries(seed, n, start, drift, vol) {
  let v = start, s = seed; const out = [];
  for (let i = 0; i < n; i++) { s = (s * 9301 + 49297) % 233280; v = v * (1 + drift + (s / 233280 - .5) * vol); out.push(v); }
  return out;
}
const D_RANGES = ["1D", "5D", "1M", "6M", "YTD", "1Y", "5Y"];
const D_SERIES_COLORS = ["#5E6AD2", "#F2994A", "#8FB3E8", "#E8C46A"];
function dPlot(series, o = {}) {
  const W = 620, H = 130, padB = 18, padR = 46;
  const all = series.flatMap(s => s.v), lo = Math.min(...all), hi = Math.max(...all), span = hi - lo || 1;
  const n = series[0].v.length, X = i => i / (n - 1) * (W - padR), Y = v => 6 + (1 - (v - lo) / span) * (H - padB - 12);
  const grid = [0, .5, 1].map(f => { const v = lo + span * f, y = Y(v); return `<line x1="0" x2="${W - padR}" y1="${y}" y2="${y}" stroke="#222326" stroke-dasharray="2 4"/><text x="${W - padR + 6}" y="${y + 4}" fill="#5D6168" font-size="10.5">${o.fmt ? o.fmt(v) : v.toFixed(0)}</text>`; }).join("");
  const xl = (o.xLabels || []).map((l, i, a) => `<text x="${i / (a.length - 1) * (W - padR)}" y="${H - 2}" fill="#5D6168" font-size="10.5" text-anchor="${i === 0 ? "start" : i === a.length - 1 ? "end" : "middle"}">${l}</text>`).join("");
  let marks;
  if (o.bars) {
    const bw = (W - padR) / n * .62;
    marks = series[0].v.map((v, i) => { const x = (i + .5) / n * (W - padR) - bw / 2, y = Y(Math.max(v, lo)); const y0 = Y(lo); return `<rect x="${x}" y="${Math.min(y, y0 - 2)}" width="${bw}" height="${Math.max(2, y0 - y)}" rx="3" fill="${i === o.hover ? "#4CB782" : "#5E6AD2"}"/>`; }).join("");
  } else {
    marks = series.map((s, k) => {
      const c = s.color || D_SERIES_COLORS[k], d = s.v.map((v, i) => `${i ? "L" : "M"}${X(i).toFixed(1)},${Y(v).toFixed(1)}`).join("");
      const fill = o.area && k === 0 ? `<path d="${d}L${X(n - 1)},${H - padB}L0,${H - padB}Z" fill="url(#dg${k})"/>` : "";
      return `<defs><linearGradient id="dg${k}" x1="0" x2="0" y1="0" y2="1"><stop offset="0" stop-color="${c}" stop-opacity=".35"/><stop offset="1" stop-color="${c}" stop-opacity="0"/></linearGradient></defs>${fill}<path d="${d}" fill="none" stroke="${c}" stroke-width="1.8" stroke-linejoin="round" ${s.off ? 'opacity=".15"' : ""}/>`;
    }).join("");
  }
  const base = o.base != null ? `<line x1="0" x2="${W - padR}" y1="${Y(o.base)}" y2="${Y(o.base)}" stroke="#8A8F98" stroke-dasharray="1 3"/>` : "";
  const h = o.hover != null && !o.bars ? `<line x1="${X(o.hover)}" x2="${X(o.hover)}" y1="0" y2="${H - padB}" stroke="#8A8F98"/>${series.filter(s => !s.off).map((s, k) => `<circle cx="${X(o.hover)}" cy="${Y(s.v[o.hover])}" r="3.5" fill="${s.color || D_SERIES_COLORS[k]}" stroke="#151617" stroke-width="2"/>`).join("")}` : "";
  return `<svg viewBox="0 0 ${W} ${H}" preserveAspectRatio="none" role="img" aria-label="${o.label || "Chart"}">${grid}${base}${marks}${h}${xl}</svg>`;
}
D.dChart = () => {
  const st = S.d.chart || "stock", hover = st === "hover";
  const fmt = v => "$" + v.toFixed(0);
  if (st === "compare") {
    const s = [{ n: "AAPL", v: dSeries(7, 60, 100, .0012, .03) }, { n: "MSFT", v: dSeries(11, 60, 100, .0004, .028) }, { n: "NVDA", v: dSeries(23, 60, 100, .003, .05) }]
      .map(x => ({ ...x, v: x.v.map(y => y / x.v[0] * 100 - 100) }));
    const off = S.d.chartOff || "";
    const plot = dPlot(s.map(x => ({ v: x.v, off: off === x.n })), { fmt: v => (v > 0 ? "+" : "") + v.toFixed(0) + "%", base: 0, xLabels: ["Apr", "Jun", "Aug", "Oct"], hover: 44, label: "AAPL, MSFT and NVDA over 6 months" });
    return deskScene(dBar(D_ASK + `<div class="dbody">${dYou("Compare Apple, Microsoft and Nvidia over the last six months")}
      ${dFold(["Stocks: AAPL, MSFT, NVDA"])}
      <div class="dcoderen agent md">Nvidia led with <b>+31%</b>, Apple gained <b>+9%</b>, and Microsoft was nearly flat at <b>+2%</b>.</div>
      <div class="dchart"><div class="ch"><span class="tt"><b>AAPL vs MSFT vs NVDA</b><small>Change since Apr 14, in %</small></span><span class="ranges">${D_RANGES.map(r => `<button aria-pressed="${r === "6M"}">${r}</button>`).join("")}</span></div>
        <div class="plot">${plot}<div class="tipc" style="left:${44 / 59 * 88}%">Sep 2 <span>·</span> AAPL +6.1% <span>·</span> MSFT +0.8% <span>·</span> NVDA +24.3%</div></div>
        <div class="foot"><span class="legend">${s.map((x, k) => `<button aria-pressed="${off !== x.n}" data-dact="chartOff:${x.n}"><i style="background:${D_SERIES_COLORS[k]}"></i>${x.n}</button>`).join("")}</span><span class="src">Yahoo Finance</span></div></div>
    </div>` + dFollow + dHints()));
  }
  if (st === "bars") {
    const v = [412, 388, 455, 501, 476, 530, 562];
    const plot = dPlot([{ v }], { bars: true, fmt: v => "€" + v.toFixed(0), xLabels: ["Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct"], hover: 6, label: "Spending per month" });
    return deskScene(dBar(D_ASK + `<div class="dbody">${dYou("Chart my grocery spending per month")}
      ${dFold(["Gmail: search receipts"])}
      <div class="dcoderen agent md">Groceries went from <b>€412</b> in April to <b>€562</b> in October, mostly from bigger weekend shops.</div>
      <div class="dchart"><div class="ch"><span class="tt"><b>Groceries per month</b><small>From 64 receipts in Gmail</small></span></div>
        <div class="plot">${plot}<div class="tipc" style="left:85%">Oct <span>·</span> €562</div></div>
        <div class="foot"><span>EUR</span></div></div>
    </div>` + dFollow + dHints()));
  }
  const loading = st === "loading", range = loading ? "1Y" : "1M";
  const v = dSeries(5, 22, 228, .0019, .025), last = v[v.length - 1], prev = v[0] * .995, ch = last - prev;
  const plot = dPlot([{ v }], { area: true, fmt, base: prev, hover: hover ? 15 : null, xLabels: ["Sep 12", "Sep 22", "Oct 2", "Oct 10"], label: "AAPL over 1 month" });
  const tip = hover ? `<div class="tipc" style="left:${15 / 21 * 92}%">Oct 3 <span>·</span> $${v[15].toFixed(2)}</div>` : "";
  return deskScene(dBar(D_ASK + `<div class="dbody">${dYou("How is Apple stock doing?")}
    ${dFold(["Stocks: AAPL"])}
    <div class="dcoderen agent md">Apple closed at <b>$${last.toFixed(2)}</b>, up <b>${(ch / prev * 100).toFixed(1)}%</b> over the past month, after the iPhone Duo preorders opened.</div>
    <div class="dchart"><div class="ch"><span class="tt"><b>AAPL</b><small>Apple Inc. · NASDAQ · USD</small></span><span class="px"><b>$${last.toFixed(2)}</b><small class="up">+${ch.toFixed(2)} (+${(ch / prev * 100).toFixed(2)}%) ${loading ? "1M" : range}</small></span></div>
      <div class="plot${loading ? " wait" : ""}">${plot}${tip}${loading ? `<div style="position:absolute;inset:0;display:grid;place-items:center"><span class="dspin"></span></div>` : ""}</div>
      <div class="foot"><span class="ranges">${D_RANGES.map(r => `<button aria-pressed="${r === range}" data-dact="chartRange">${r}</button>`).join("")}</span><span class="src">Yahoo Finance</span></div></div>
  </div>` + dFollow + dHints()));
};
VARIANTS.dChart = [
  ["Stock", () => { S.d.chart = "stock"; render(); }, () => (S.d.chart || "stock") === "stock"],
  ["Hover", () => { S.d.chart = "hover"; render(); }, () => S.d.chart === "hover"],
  ["Range loading", () => { S.d.chart = "loading"; render(); }, () => S.d.chart === "loading"],
  ["Compare", () => { S.d.chart = "compare"; render(); }, () => S.d.chart === "compare"],
  ["Bar chart", () => { S.d.chart = "bars"; render(); }, () => S.d.chart === "bars"],
];
DESK_SCREENS[0].items.push(["dRerun", "Rerun (admin)", "PromptBar, rerun"], ["dChart", "Chart in a reply", "ChartCard"]);
