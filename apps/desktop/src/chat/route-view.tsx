import { useEffect, useMemo, useState } from "react";
import { invoke } from "../ui/tauri.ts";
import { ROUTE_CHEV, ROUTE_WALK, routeChanges, routeSketch, type RouteCard, type RouteLeg, type RoutePoint } from "./routes.ts";

export function RouteView({ callId }: { callId: string }) {
  const [route, setRoute] = useState<RouteCard | null>(null), [failed, setFailed] = useState(false);
  const [picked, setPicked] = useState(0), [open, setOpen] = useState<Set<string>>(() => new Set());
  useEffect(() => {
    let active = true;
    void invoke<RouteCard>("route_card", { callId }).then(value => { if (active) setRoute(value); }, () => { if (active) setFailed(true); });
    return () => { active = false; };
  }, [callId]);
  const current = route?.routes[picked] ?? route?.routes[0] ?? null;
  const sketch = useMemo(() => current ? routeSketch(current) : "", [current]);
  if (failed) return <div className="dmuted">Route no longer available</div>;
  if (!route) return <div className="dmuted"><span className="dspin" />Loading the route</div>;
  if (!current) return <div className="dmuted">No route found</div>;
  const toggle = (key: string) => setOpen(previous => { const next = new Set(previous); if (!next.delete(key)) next.add(key); return next; });
  const maps = () => {
    const points = current.legs.flatMap(leg => leg.points);
    if (!points.length) return;
    const at = (point: RoutePoint) => `${point.latitude},${point.longitude}`;
    void invoke("open_url", { url: `https://maps.apple.com/?saddr=${at(points[0])}&daddr=${at(points[points.length - 1])}&dirflg=r` }).catch(() => {});
  };
  return <><div className="ch"><span className="tt"><b>{`${current.departs} to ${current.arrives} · ${current.minutes} min${routeChanges(current, " · ")}`}</b><small>{current.live ? "Live times" : "Timetable times, delays not included"}</small></span></div>
    {route.routes.length > 1 && <div className="foot"><span className="ranges">{route.routes.map((option, index) => <button key={index} data-route={index} aria-pressed={index === picked} onClick={event => { event.stopPropagation(); setPicked(index); }}>{option.minutes} min{routeChanges(option, ", ")}</button>)}</span></div>}
    <div className="rs" dangerouslySetInnerHTML={{ __html: sketch }} /><div className="rlegs">{current.legs.map((leg, index) => <Leg key={`${picked}:${index}`} leg={leg} id={`${picked}:${index}`} open={open.has(`${picked}:${index}`)} toggle={toggle} />)}</div>
    <div className="foot"><button className="dbtn" data-maps onClick={event => { event.stopPropagation(); maps(); }}>Open in Maps</button><span className="src">Transitous</span></div></>;
}
function Station({ code, color }: { code: string | undefined; color: string }) {
  return code ? <span className="rc" style={{ borderColor: color }}>{code}</span> : null;
}
function Leg({ leg, id, open, toggle }: { leg: RouteLeg; id: string; open: boolean; toggle(key: string): void }) {
  if (leg.mode === "walk") return <div className="dmuted"><span className="rw" dangerouslySetInnerHTML={{ __html: ROUTE_WALK }} />Walk {leg.minutes} min{leg.meters >= 1000 ? `, ${(leg.meters / 1000).toFixed(1)} km` : ""} to {leg.to || "destination"}</div>;
  const color = leg.color || "var(--accent)", badge = leg.line || leg.name || leg.mode, between = leg.between ?? [];
  const label = `${leg.stops} stop${leg.stops > 1 ? "s" : ""}`;
  return <div className="rl"><span className="rb" style={{ background: color, color: leg.text_color || "var(--ink)" }}>{badge}</span><span className="rt" style={{ borderColor: color }}><b>{leg.name || badge}{leg.headsign && <> <span className="m">toward {leg.headsign}</span></>}</b><span><b className="n">{leg.departs}</b> <Station code={leg.from_code} color={color} />{leg.from}{leg.from_track && `, platform ${leg.from_track}`}{!!leg.stops && (between.length ? <> <button className="rst m" data-stops={id} aria-expanded={open} onClick={event => { event.stopPropagation(); toggle(id); }}>· <span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: ROUTE_CHEV }} />{label} ·</button></> : <> <span className="m">· {label} ·</span></>)} <b className="n">{leg.arrives}</b> <Station code={leg.to_code} color={color} />{leg.to || "destination"}</span>{open && <span className="rsl">{between.map((stop, index) => <span key={index}><span className="n">{stop.time}</span><Station code={stop.code} color={color} />{stop.name}</span>)}</span>}</span></div>;
}
