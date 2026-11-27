// A session's plan above the message field: one line with how far it is and
// the step that runs, which opens to the steps. A step that subagents ran
// under opens to their rows, which open the subagent as they do in the log.
import { Spinner } from "../../ui/kit.tsx";
import { Icon } from "../../ui/icons.tsx";
import { plural } from "../../ui/format.ts";
import { SubagentsView, type SubagentRow } from "./subagents-view.tsx";

/// A step of the plan: how many subagents ran under it, and their rows once
/// it is opened.
export interface PlanRow { status: string; content: string; count: number; subs: readonly SubagentRow[] | null }

interface PlanProps {
  plan: readonly PlanRow[];
  open: boolean;
  /// The session waits for an answer, so nothing turns.
  asking: boolean;
  toggle: () => void;
  toggleStep: (index: number) => void;
}

export function PlanTray({ plan, open, asking, toggle, toggleStep }: PlanProps) {
  const done = plan.filter(item => item.status === "completed").length;
  const running = plan.find(item => item.status === "in_progress");
  // Keeps the field focused, so the keyboard and the composer stay where they are.
  const keepFocus = (event: React.MouseEvent) => event.preventDefault();

  return <>
    <button type="button" className="ag-plan" aria-expanded={open} onMouseDown={keepFocus} onClick={toggle}>
      {running && !asking ? <Spinner dot /> : <span className={done === plan.length ? "ddot" : "ddot off"} />}
      <span>Plan {done}/{plan.length}{running && <b>{running.content}</b>}</span>
      <Icon name="chev" />
    </button>
    {open && <div className="ag-steps">{plan.map((item, index) => <Step key={index} item={item} asking={asking} toggle={() => toggleStep(index)} />)}</div>}
  </>;
}

function Step({ item, asking, toggle }: { item: PlanRow; asking: boolean; toggle: () => void }) {
  const mark = item.status === "completed" ? <span className="dok"><Icon name="check" /></span> : item.status === "in_progress" && !asking ? <Spinner dot /> : <span className={item.status === "in_progress" ? "ddot hot" : "ddot off"} />;
  const line = <><span className="dmark">{mark}</span><span className={item.status === "pending" ? "t dmut" : "t"}>{item.content}</span></>;
  if (!item.count) return <div className="it">{line}</div>;

  return <>
    <button type="button" className="it" aria-expanded={item.subs !== null} onMouseDown={event => event.preventDefault()} onClick={toggle}>{line}<small>{plural(item.count, "agent", "agents")}</small><Icon name="chev" /></button>
    {item.subs && <div className="subs"><SubagentsView rows={item.subs} /></div>}
  </>;
}
