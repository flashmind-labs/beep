import { Button } from "../ui/kit.tsx";
import { Fragment, useLayoutEffect, useRef, useState } from "react";
import { agentLabel } from "../project/projects-state.ts";
import { creditAmount, creditScope, providerName } from "../ui/credits.ts";
import { harnessMark, providerMark } from "../ui/icons.tsx";
import { money, tokenCount } from "../ui/format.ts";
import { useDialogRef } from "../ui/parts.tsx";
import { asOf, FIVE_HOURS, harnessCost, harnessName, modelTotals, PERIODS, pillPlans, planName, resetName, shortName, showsDollars, todayLine, WARN_USED, windowName, windowUsed, type PeriodKey, type PlanLimits, type UsageTableRow, type UsageView } from "./usage-state.ts";

function Bar({ used }: { used: number }) { return <span className="bar"><i className={used > WARN_USED ? "warn" : ""} style={{ width: `${used}%` }} /></span>; }
/// A harness's mark, else its short name: never a letter tile.
function Mark({ agent }: { agent: string }) {
  const mark = harnessMark(agent);
  return mark ? <span className="us-mk">{mark}</span> : <span>{shortName(agent)}</span>;
}
function Provider({ provider }: { provider: string }) {
  const mark = providerMark(provider);
  return mark ? <span className="us-mk">{mark}</span> : <span>{providerName(provider)}</span>;
}
function Limits({ plan, now }: { plan: PlanLimits; now: number }) {
  const credits = plan.credits ?? [], single = !plan.windows.length && credits.length === 1;
  const name = [agentLabel(plan.agent), plan.account, planName(plan.agent, plan.plan), single ? `${providerName(credits[0].provider)} credits` : ""].filter(Boolean).join(" · ");
  return <section className="us-card"><h5 title={`Read on ${plan.machine_name}`}><span>{harnessMark(plan.agent) && <Mark agent={plan.agent} />}{name}</span><span>{asOf(plan.as_of, now)}</span></h5>
    {plan.windows.map((window, index) => { const used = windowUsed(window), reset = window.resets_at != null ? ` · resets ${resetName(window.resets_at, now)}` : ""; return <div className="us-lim" key={index}><span>{windowName(window)}</span><small>{used}% used{reset}</small><Bar used={used} /></div>; })}
    {credits.map((credit, index) => <Fragment key={index}><div className="us-row"><span>{single ? creditScope(credit.scope) : `${providerName(credit.provider)} · ${creditScope(credit.scope)}`}</span><span>{credit.remaining_full ?? creditAmount(credit, credit.remaining)} remaining</span></div><div className="us-row"><small>{credit.resets ? `Resets ${credit.resets}` : ""}</small><small>{credit.used_full ?? creditAmount(credit, credit.used)} used · {credit.total_full ?? creditAmount(credit, credit.total)} total</small></div>{credit.provider === "openrouter" && credit.scope === "api_key" && credit.remaining == null && <div className="us-note">Account balance unavailable.</div>}</Fragment>)}
  </section>;
}
const shortDate = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", timeZone: "UTC" });

function sessionDates(row: UsageTableRow) {
  const first = row.first && new Date(row.first), last = row.last && new Date(row.last);
  if (first && last && first <= last) return row.first === row.last ? shortDate.format(first) : shortDate.formatRange(first, last);
  if (first) return `From ${shortDate.format(first)}`;
  if (last) return `Through ${shortDate.format(last)}`;
  return "Unknown";
}

function ProjectTable({ rows, historical = false, expanded, toggle }: {
  rows: UsageTableRow[]; historical?: boolean; expanded: boolean; toggle(): void;
}) {
  const shown = expanded ? rows : rows.slice(0, 5);
  return <>
    <table className="us-table">
      <caption>{historical ? "Historical spend (daily split unknown)" : "Daily usage"}</caption>
      <thead><tr>
        <th scope="col">{historical ? "Session dates" : "Date"}</th>
        <th scope="col">Project</th>
        {historical && <th scope="col">Harness</th>}
        <th scope="col" className="num">Spend</th>
        <th scope="col" className="num">Tokens</th>
      </tr></thead>
      <tbody>{shown.map((row, i) => <tr key={i}>
        <td>{sessionDates(row)}</td>
        <td>{row.project ?? "Other projects"}</td>
        {historical && <td>{row.agent && agentLabel(row.agent)}</td>}
        <td className="num">{money(row.cost_usd)}</td>
        <td className="num">{tokenCount(row.tokens)}</td>
      </tr>)}</tbody>
    </table>
    {rows.length > 5 && <Button look="plain" className="us-more" onClick={toggle} aria-expanded={expanded}>
      {expanded ? "Show less" : `Show ${rows.length - 5} more`}
    </Button>}
  </>;
}

function Period({ view, periodKey, allProjects, pick, more }: {
  view: UsageView; periodKey: PeriodKey; allProjects: boolean; pick(key: PeriodKey): void; more(): void;
}) {
  const period = view[periodKey], daily = period.daily ?? [], historical = view.historical ?? [];
  const models = modelTotals(period.harnesses);
  const [showHistory, setShowHistory] = useState(false);
  const hasSpend = period.tokens > 0 || period.total.cost_usd > 0;

  return <section className="us-card us-spend">
    <div className="us-seg">{PERIODS.map(p =>
      <Button look="plain" key={p.key} className={p.key === periodKey ? "on" : ""} aria-pressed={p.key === periodKey} onClick={() => pick(p.key)}>{p.label}</Button>
    )}</div>
    {daily.length > 0
      ? <ProjectTable rows={daily} expanded={allProjects} toggle={more} />
      : periodKey === "today" && <div className="us-row"><small>{hasSpend ? "Daily breakdown unavailable" : "Nothing yet today"}</small></div>}
    {(period.harnesses.length > 0 || hasSpend) && <table className="us-table">
      <caption>By harness</caption>
      <thead><tr><th scope="col">Harness</th><th scope="col" className="num">Spend</th><th scope="col" className="num">Tokens</th></tr></thead>
      <tbody>{period.harnesses.map(spend => <tr key={`${spend.agent}:${spend.subscription}`}>
        <td>{harnessName(spend)}</td><td className="num">{harnessCost(spend)}</td><td className="num">{tokenCount(spend.tokens)}</td>
      </tr>)}</tbody>
      <tfoot><tr><th scope="row">Total</th><td className="num">{money(period.total.cost_usd)}</td><td className="num">{tokenCount(period.tokens)}</td></tr></tfoot>
    </table>}
    {models.length > 0 && <table className="us-table">
      <caption>By model</caption>
      <thead><tr><th scope="col">Model</th><th scope="col" className="num">Tokens</th></tr></thead>
      <tbody>{models.map(model => <tr key={model.name}><td>{model.name}</td><td className="num">{tokenCount(model.tokens)}</td></tr>)}</tbody>
    </table>}
    {historical.length > 0 && <ProjectTable rows={historical} historical expanded={showHistory} toggle={() => setShowHistory(!showHistory)} />}
  </section>;
}
export function UsageWidget({ view, open, period, allProjects, toggle, pick, more }: { view: UsageView; open: boolean; period: PeriodKey; allProjects: boolean; toggle(): void; pick(key: PeriodKey): void; more(): void }) {
  const pop = useRef<HTMLDivElement>(null), dialog = useDialogRef(pop), shown = view[period] ?? view.today;
  const plans = pillPlans(view.limits).map(plan => {
    const name = [agentLabel(plan.agent), plan.account, plan.model].filter(Boolean).join(" ");
    const readings = plan.windows.map(window => `${windowUsed(window)}% used (${windowName(window)})`);
    return { ...plan, label: `${name} ${readings.join(", ")}` };
  });
  const label = plans.length ? `Plan usage: ${plans.map(plan => plan.label).join(" · ")}` : `Cost today: ${todayLine(view.today)}`;
  // The pop moves back inside the window when its edge would leave it, and scrolls when taller than the room below.
  const [place, setPlace] = useState({ shift: 0, maxHeight: 0 });
  useLayoutEffect(() => {
    const el = pop.current;
    if (!el) return;
    const r = el.getBoundingClientRect(), gap = 8, left = r.left - place.shift, right = r.right - place.shift;
    const shift = right > innerWidth - gap ? innerWidth - gap - right : left < gap ? gap - left : 0;
    const maxHeight = Math.max(160, innerHeight - r.top - gap);
    if (shift !== place.shift || maxHeight !== place.maxHeight) setPlace({ shift, maxHeight });
  }, [view, open, period, allProjects, place]);
  return <><button type="button" className={`us-pill${plans.length ? " plans" : ""}`} title="Plan usage and cost" aria-label={label} aria-expanded={open} onClick={toggle}>
      {plans.map(plan => <span className="us-plan" key={`${plan.agent}:${plan.account ?? ""}:${plan.model ?? ""}`} title={plan.label}>
        {plan.model ?? <Mark agent={plan.agent} />}{plan.account && !plan.model && <small>{plan.account}</small>}
        <span className="us-readings">{plan.windows.map(window => {
          const used = windowUsed(window);
          return <span className="us-reading" key={window.minutes}>
            {window.minutes === FIVE_HOURS && <small className="us-window">5h</small>}
            <b className={used > WARN_USED ? "warn" : ""}>{used}%</b>
          </span>;
        })}</span>
      </span>)}
      {view.limits.flatMap(plan => plan.credits ?? []).map((credit, n) => credit.remaining != null && <span className="us-x" key={n} title={`${providerName(credit.provider)}: ${credit.remaining_full ?? creditAmount(credit, credit.remaining)} remaining`}><Provider provider={credit.provider} /><span><b>{credit.remaining_short ?? creditAmount(credit, credit.remaining)}</b> left</span></span>)}
      {(!plans.length || view.today.total.cost_usd > 0) && <span className="us-x"><span><b>{todayLine(view.today)}</b> today</span></span>}
    </button>
    {open && <div ref={dialog} className="us-pop" style={{ transform: place.shift ? `translateX(${place.shift}px)` : undefined, maxHeight: place.maxHeight || undefined }} role="dialog" aria-label="Plan usage and cost">{view.limits.filter(plan => plan.windows.length || plan.credits?.length).map((plan, i) => <Limits key={i} plan={plan} now={Date.now()} />)}<Period view={view} periodKey={period} allProjects={allProjects} pick={pick} more={more} />{showsDollars(shown) && <div className="us-note">Dollars are API spend. ~ is what plan use would cost on the API, not in the total.</div>}</div>}
  </>;
}
