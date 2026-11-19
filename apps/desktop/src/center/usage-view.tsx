import { Fragment, useLayoutEffect, useRef, useState } from "react";
import { agentLabel } from "../project/projects-state.ts";
import { creditAmount, creditScope, providerName } from "../ui/credits.ts";
import { harnessMark } from "../ui/icons.tsx";
import { money, tokenCount } from "../ui/format.ts";
import { useDialogRef } from "../ui/parts.tsx";
import { asOf, harnessLine, harnessName, modelTotals, PERIODS, pillPlans, planName, projectExtra, resetName, shortName, showsDollars, todayLine, WARN_USED, windowName, windowUsed, type PeriodKey, type PlanLimits, type UsageView } from "./usage.tsx";

function Bar({ used }: { used: number }) { return <span className="bar"><i className={used > WARN_USED ? "warn" : ""} style={{ width: `${used}%` }} /></span>; }
/// A harness's mark, else its short name: never a letter tile.
function Mark({ agent }: { agent: string }) {
  const mark = harnessMark(agent);
  return mark ? <span className="us-mk">{mark}</span> : <span>{shortName(agent)}</span>;
}
function Limits({ plan, now }: { plan: PlanLimits; now: number }) {
  const credits = plan.credits ?? [], single = !plan.windows.length && credits.length === 1;
  const name = [agentLabel(plan.agent), plan.account, planName(plan.agent, plan.plan), single ? `${providerName(credits[0].provider)} credits` : ""].filter(Boolean).join(" · ");
  return <div><h5 title={`Read on ${plan.machine_name}`}><span>{harnessMark(plan.agent) && <Mark agent={plan.agent} />}{name}</span><span>{asOf(plan.as_of, now)}</span></h5>
    {plan.windows.map((window, index) => { const used = windowUsed(window), reset = window.resets_at != null ? ` · resets ${resetName(window.resets_at, now)}` : ""; return <div className="us-lim" key={index}><span>{windowName(window)}</span><Bar used={used} /><small>{used}% used{reset}</small></div>; })}
    {credits.map((credit, index) => <Fragment key={index}><div className="us-row"><span>{single ? creditScope(credit.scope) : `${providerName(credit.provider)} · ${creditScope(credit.scope)}`}</span><span>{credit.remaining_full ?? creditAmount(credit, credit.remaining)} remaining</span></div><div className="us-row"><small>{credit.resets ? `Resets ${credit.resets}` : ""}</small><small>{credit.used_full ?? creditAmount(credit, credit.used)} used · {credit.total_full ?? creditAmount(credit, credit.total)} total</small></div>{credit.provider === "openrouter" && credit.scope === "api_key" && credit.remaining == null && <div className="us-note">Account balance unavailable.</div>}</Fragment>)}
  </div>;
}
function SpendRow({ name, cost, extra, strong = false, title }: { name: React.ReactNode; cost: string; extra: string; strong?: boolean; title?: string }) {
  const Tag = strong ? "b" : "span";
  return <div className="us-row" title={title}>{typeof name === "string" ? <Tag>{name}</Tag> : name}<Tag className="us-r">{cost}{extra && <small>{cost ? " · " : ""}{extra}</small>}</Tag></div>;
}
function Period({ view, periodKey, allProjects, pick, more }: { view: UsageView; periodKey: PeriodKey; allProjects: boolean; pick(key: PeriodKey): void; more(): void }) {
  const period = view[periodKey], shown = allProjects ? period.projects : period.projects.slice(0, 5), extra = period.projects.length - 5;
  const models = modelTotals(period.harnesses), tokens = period.tokens ? tokenCount(period.tokens) : "", paid = period.total.cost_usd > 0 || !tokens;
  return <div><div className="us-seg">{PERIODS.map(p => <button type="button" key={p.key} className={p.key === periodKey ? "on" : ""} aria-pressed={p.key === periodKey} onClick={() => pick(p.key)}>{p.label}</button>)}</div>
    {period.projects.length ? <><h5><span>By project</span></h5>{shown.map((spend, i) => <SpendRow key={i} name={spend.project} cost={spend.cost_usd > 0 ? money(spend.cost_usd) : ""} extra={projectExtra(spend)} />)}{extra > 0 && <button type="button" className="us-more" onClick={more}>{allProjects ? "Show less" : `Show ${extra} more`}</button>}</> : periodKey === "today" && <div className="us-row"><small>Nothing yet today</small></div>}
    {!!period.harnesses.length && <><h5 className={period.projects.length ? "us-sub" : ""}><span>By harness</span></h5>{period.harnesses.map((spend, i) => <SpendRow key={i} title={spend.models.length ? spend.models.map(m => `${m.name}: ${tokenCount(m.tokens)} tokens`).join("\n") : undefined} name={<small>{harnessName(spend)}</small>} cost="" extra={harnessLine(spend)} />)}</>}
    {!!models.length && <><h5 className="us-sub"><span>By model</span></h5>{models.map(model => <SpendRow key={model.name} name={<small>{model.name}</small>} cost="" extra={tokenCount(model.tokens)} />)}</>}
    {(period.total.cost_usd > 0 || !!tokens) && <SpendRow name="Total" cost={paid ? money(period.total.cost_usd) : ""} extra={tokens} strong />}
  </div>;
}
export function UsageWidget({ view, open, period, allProjects, toggle, pick, more }: { view: UsageView; open: boolean; period: PeriodKey; allProjects: boolean; toggle(): void; pick(key: PeriodKey): void; more(): void }) {
  const pop = useRef<HTMLDivElement>(null), dialog = useDialogRef(pop), plans = pillPlans(view.limits), shown = view[period] ?? view.today;
  const label = plans.length ? `Plan usage: ${plans.map(plan => `${plan.model ?? agentLabel(plan.agent)} ${plan.used}%`).join(", ")}` : `Cost today: ${todayLine(view.today)}`;
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
      {plans.map(plan => <span className="us-plan" key={`${plan.agent}:${plan.account ?? ""}:${plan.model ?? ""}`} title={`${[agentLabel(plan.agent), plan.account].filter(Boolean).join(" ")}: ${plan.used}% of ${plan.window} used`}>{plan.model ?? <Mark agent={plan.agent} />}{plan.account && !plan.model && <small>{plan.account}</small>}<b className={plan.used > WARN_USED ? "warn" : ""}>{plan.used}%</b></span>)}
      {view.limits.flatMap(plan => plan.credits ?? []).map((credit, n) => credit.remaining != null && <span className="us-x" key={n}>{providerName(credit.provider)} <b>{credit.remaining_short ?? creditAmount(credit, credit.remaining)}</b> left</span>)}
      {(!plans.length || view.today.total.cost_usd > 0) && <span className="us-x"><b>{todayLine(view.today)}</b> today</span>}
    </button>
    {open && <div ref={dialog} className="us-pop" style={{ transform: place.shift ? `translateX(${place.shift}px)` : undefined, maxHeight: place.maxHeight || undefined }} role="dialog" aria-label="Plan usage and cost">{view.limits.filter(plan => plan.windows.length || plan.credits?.length).map((plan, i) => <Limits key={i} plan={plan} now={Date.now()} />)}<Period view={view} periodKey={period} allProjects={allProjects} pick={pick} more={more} />{showsDollars(shown) && <div className="us-note">Dollars are API spend. ~ is what plan use would cost on the API, not in the total.</div>}</div>}
  </>;
}
