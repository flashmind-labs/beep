// A report header's challenge: Challenge, with the harness and model to
// challenge on, then the challenger at work with Stop, then what it found.
import { useMemo, useReducer, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { useStore } from "zustand";
import { Button, Spinner } from "../ui/kit.tsx";
import { plural, reason } from "../ui/format.ts";
import { AG, loadAgentHosts } from "./agents.ts";
import type { Picked } from "./agentpicker.tsx";
import { byPreference } from "./agentpick.ts";
import { pickPopover } from "./pick-popover.ts";
import { challengeReport, openChallenger, stopChallenger } from "./challenge.ts";
import { challengeDefault, challengeOf, challenges, keepChallengeModel, lastChallengeModel } from "./challenge-state.ts";
import { reviews } from "./review-state.ts";

/// `can`: the report may be challenged now.
export function ChallengeActs({ entry, can }: { entry: string; can: boolean }) {
  const key = useStore(reviews, state => state.open);
  const challenge = useStore(challenges, state => challengeOf(state.by, key, Number(entry)));
  const [error, setError] = useState("");
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
      // On the models of the harness picked last, else of another harness than the session's.
      onModels: true,
      opening: () => {
        const offered = [...host()?.agents ?? []].sort(byPreference);
        Object.assign(picked, challengeDefault(offered, AG.page?.session.agent ?? "", lastChallengeModel()));
        return offered.length > 0;
      },
      start: () => {
        const pick = { agent: picked.agent, model: picked.model, effort: picked.effort, fast: picked.fast };
        keepChallengeModel(pick);
        setError("");
        challengeReport(Number(entry), pick).catch(e => setError(reason(e)));
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
    await stopChallenger(key, challenge).catch(e => setError(reason(e)));
    setStopping(false);
  };

  if (challenge?.state === "running") return <span className="report-busy">
    <Spinner />{stopping ? "Stopping…" : "Challenging…"}
    <Button small look="quiet" onClick={() => void openChallenger(challenge)} title="Open the challenger's session">Open</Button>
    <Button small look="quiet" danger disabled={stopping} onClick={() => void stop()} title="Stop the challenger and remove its session and copy">Stop</Button>
  </span>;
  if (challenge) return <Button small look="quiet" onClick={() => void openChallenger(challenge)} title="Open the challenger's session">{challenge.findings ? plural(challenge.findings, "finding", "findings") : "Nothing broken"}</Button>;
  if (!can) return null;

  return <span className="report-challenge" ref={pop.setBox}>
    {error && <span className="report-err" title={error}>Not started</span>}
    <Button small look="quiet" ref={pop.setButton} aria-expanded={pop.open} onClick={() => void open()} title="Pick a model to try to prove this report wrong">Challenge</Button>
    <div ref={pop.setSlot}>{pop.view}</div>
  </span>;
}
