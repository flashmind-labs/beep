// A report header's challenge: Challenge, with the harness and model to
// challenge on, then the challenger at work with Stop, then what it found.
import { useMemo, useReducer, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { createStore, useStore } from "zustand";
import { Button, Spinner } from "../ui/kit.tsx";
import { plural, reason } from "../ui/format.ts";
import { AG, loadAgentHosts } from "./agents.ts";
import type { Picked } from "./agentpicker.tsx";
import { pickPopover } from "./pick-popover.ts";
import { challengeReport, openChallenger, stopChallenger } from "./challenge.ts";
import { challengeOf, challenges, type Challenge } from "./challenge-state.ts";
import { reviews } from "./review-state.ts";

/// What the machine refused just now for a report's challenge, by
/// `machine:session:entry`: a start (`about` null), or a stop of the
/// challenger `about`. It holds while that challenger still runs.
interface Problem { label: string; why: string; about: string | null }

const problems = createStore<Readonly<Record<string, Problem>>>(() => ({}));

/// `problem`, while the challenge it was about is still where it was.
const current = (problem: Problem | null, challenge: Challenge | null) =>
  problem && problem.about === (challenge?.challenger.session ?? null) && challenge?.state !== "done" && challenge?.state !== "failed" ? problem : null;

const problemKey = (key: string | null, entry: string) => `${key}:${entry}`;

/// `can`: the report may be challenged now.
export function ChallengeActs({ entry, can }: { entry: string; can: boolean }) {
  const key = useStore(reviews, state => state.open);
  const challenge = useStore(challenges, state => challengeOf(state.by, key, Number(entry)));
  const setProblem = (next: Problem | null) => problems.setState(state => {
    const { [problemKey(key, entry)]: _, ...rest } = state;
    return next ? { ...rest, [problemKey(key, entry)]: next } : rest;
  }, true);
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
        challengeReport(Number(entry), pick).catch(e => setProblem({ label: "Not started", why: reason(e), about: null }));
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
    await stopChallenger(key, challenge).catch(e => setProblem({ label: "Not stopped", why: reason(e), about: challenge.challenger.session }));
    setStopping(false);
  };
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
        {openButton(challenge)}
        <Button small look="quiet" danger disabled={stopping} onClick={() => void stop()} title="Stop the challenger and remove its session and copy">Stop</Button>
      </span>;
    case "failed":
      return <span className="report-busy" data-challenge="failed">
        <span className="report-err">Challenge failed</span>
        {openButton(challenge)}{can && picker("Again")}
      </span>;
    case "done":
      return <>
        {challenge.findings
          ? <Button small look="quiet" onClick={event => showFindings(event.currentTarget)} title="Go to the findings">{plural(challenge.findings, "finding", "findings")}</Button>
          : <span className="report-done">Nothing broken</span>}
        {challenge.held.length > 0 && <span className="report-done" title={`Held: ${challenge.held.join(", ")}`}>{challenge.held.length} held</span>}
      </>;
    case undefined:
      return can ? picker("Challenge") : null;
  }
}

/// Scrolls to the first finding on the report `from` is in.
function showFindings(from: HTMLElement) {
  from.closest(".dcoderen.report")?.querySelector(".rv-note[data-ask='finding']")?.scrollIntoView({ block: "center", behavior: "smooth" });
}

/// Why a report's challenge failed, or what the machine refused, in words
/// under the report's header.
export function ChallengeNote({ entry }: { entry: string }) {
  const key = useStore(reviews, state => state.open);
  const challenge = useStore(challenges, state => challengeOf(state.by, key, Number(entry)));
  const problem = current(useStore(problems, state => state[problemKey(key, entry)] ?? null), challenge);
  // A failure is named in the header already: the note says why.
  const note = problem ?? (challenge?.state === "failed" ? { label: "", why: challenge.error } : null);
  if (!note) return null;

  return <p className="report-note" role="status">{note.label && <b>{note.label}: </b>}{note.why}</p>;
}
