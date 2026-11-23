// A report header's challenge: Challenge, with the harness and model to
// challenge on, then the challenger at work with Stop, then what it found.
import { useMemo, useReducer, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { useStore } from "zustand";
import { Button, Spinner } from "../ui/kit.tsx";
import { plural, reason } from "../ui/format.ts";
import { AG, loadAgentHosts } from "./agents.ts";
import type { Picked } from "./agentpicker.tsx";
import { pickPopover } from "./pick-popover.ts";
import { challengeReport, openChallenger, stopChallenger } from "./challenge.ts";
import { challengeOf, challenges, type Challenge } from "./challenge-state.ts";
import { reviews } from "./review-state.ts";

/// `can`: the report may be challenged now.
export function ChallengeActs({ entry, can }: { entry: string; can: boolean }) {
  const key = useStore(reviews, state => state.open);
  const challenge = useStore(challenges, state => challengeOf(state.by, key, Number(entry)));
  const [problem, setProblem] = useState<{ label: string; why: string } | null>(null);
  const [stopping, setStopping] = useState(false);
  const [, redraw] = useReducer((n: number) => n + 1, 0);
  const picked = useRef<Picked>({ agent: "", model: "", effort: null, fast: null, query: "" }).current;

  const pop = useMemo(() => {
    const host = () => AG.hosts?.find(h => h.machine_id === AG.page?.session.machine_id) ?? null;
    return pickPopover({
      picked,
      // A challenger runs where the work is.
      machine: () => { const h = host(); return h ? { id: h.machine_id, name: h.machine_name, agents: h.agents } : null; },
      everywhere: () => [...new Set((AG.hosts ?? []).flatMap(h => h.agents))],
      machines: () => null,
      // The harness is the first step.
      opening: () => {
        Object.assign(picked, { agent: "", model: "", effort: null, fast: null });
        return (host()?.agents.length ?? 0) > 0;
      },
      start: () => {
        const pick = { agent: picked.agent, model: picked.model, effort: picked.effort, fast: picked.fast };
        setProblem(null);
        challengeReport(Number(entry), pick).catch(e => setProblem({ label: "Not started", why: reason(e) }));
      },
      draw: sync => { if (sync) flushSync(redraw); else redraw(); },
    });
  }, [entry, picked]);

  const open = async () => {
    if (!AG.hosts) await loadAgentHosts();
    await pop.toggle();
  };
  const stop = async () => {
    if (!challenge || !key) return;
    setStopping(true);
    setProblem(null);
    await stopChallenger(key, challenge).catch(e => setProblem({ label: "Not stopped", why: reason(e) }));
    setStopping(false);
  };
  const said = problem && <span className="report-err" title={problem.why}>{problem.label}</span>;
  const picker = (label: string) => <span className="report-challenge" ref={pop.setBox}>
    <Button small look="quiet" ref={pop.setButton} aria-expanded={pop.open} onClick={() => void open()} title="Pick a model to try to prove this report wrong">{label}</Button>
    <div ref={pop.setSlot}>{pop.view}</div>
  </span>;
  const openButton = (c: Challenge) => <Button small look="quiet" onClick={() => void openChallenger(c)} title="Open the challenger's session">Open</Button>;

  switch (challenge?.state) {
    case "running":
    case "asking":
      return <span className="report-busy" data-challenge={challenge.state}>
        {challenge.state === "running" || stopping ? <Spinner /> : <span className="report-ask-dot" />}
        <span>{stopping ? "Stopping…" : challenge.state === "running" ? "Challenging…" : "Challenger asks you"}</span>
        {said}{openButton(challenge)}
        <Button small look="quiet" danger disabled={stopping} onClick={() => void stop()} title="Stop the challenger and remove its session and copy">Stop</Button>
      </span>;
    case "failed":
      return <span className="report-busy" data-challenge="failed">
        <span className="report-err" title={challenge.error}>Challenge failed</span>
        {openButton(challenge)}{can && picker("Again")}
      </span>;
    case "done":
      return <Button small look="quiet" onClick={() => void openChallenger(challenge)} title="Open the challenger's session">{challenge.findings ? plural(challenge.findings, "finding", "findings") : "Nothing broken"}</Button>;
    case undefined:
      return can ? <>{said}{picker("Challenge")}</> : null;
  }
}
