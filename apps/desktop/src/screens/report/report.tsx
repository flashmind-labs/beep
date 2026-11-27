// report/: the "Report a problem" sheet. Main file: this one; its logic is
// in report-state.ts.
import { invoke } from "../../ui/tauri.ts";
import { type ReactNode } from "react";
import { flushSync } from "react-dom";
import { openSheet } from "../../ui/parts.tsx";
import { ReportBody } from "./report-view.tsx";
import { START, next, type Draft, type ReportEvent, type Sent } from "../../stores/report/report-state.ts";

let state = START, sheet: HTMLDivElement | null = null, close: (() => void) | null = null;
let render: ((content: ReactNode, label: string) => void) | null = null;
/// The file whose text shows under the list, and the issue body before Yes.
let opened: string | null = null, issueText: string | null = null;
/// Which opening of the sheet an answer belongs to; one that arrives after
/// the sheet closed is dropped.
let opening = 0;

/// Starts a coding agent on what was typed, from the form's "Debug with an agent".
let debug: ((text: string) => void) | null = null;

/// Opens the sheet over `host`; a second call while it is open does nothing.
/// With `onDebug` the form offers "Debug with an agent", which closes the
/// sheet and hands it what was typed.
export function openReport(host: HTMLElement, onDebug?: (text: string) => void) {
  if (sheet) return;
  debug = onDebug ?? null;
  opening++;
  state = START;
  opened = issueText = null;
  ({ sheet, close, render } = openSheet("Report a problem", "am-sheet ip-sheet", { host, canClose: () => !busy(), onClose: closed }));
  draw();
  sheet.querySelector("textarea")?.focus();
}

function closed() {
  opening++;
  render = null;
  sheet = close = null;
}

function busy() {
  return state.step === "collecting" || state.step === "sending";
}

function update(event: ReportEvent) {
  state = next(state, event);
  if (sheet) draw();
}

/// `event` for an answer asked for in opening `asked`, unless the sheet closed since.
function answer(asked: number, event: ReportEvent) {
  if (asked === opening) update(event);
}

async function collect() {
  update({ type: "collect" });
  if (state.step !== "collecting") return;
  const asked = opening;

  try {
    const draft = await invoke<Draft>("report_collect", { period: state.period });
    answer(asked, { type: "collected", draft });
  } catch (e) {
    answer(asked, { type: "failed", error: String(e) });
  }
}

async function send() {
  const draft = state.draft;
  update({ type: "send" });
  if (!draft || state.step !== "sending") return;
  const asked = opening;

  try {
    const sent = await invoke<Sent>("report_send", { draft, text: state.text });
    if (asked !== opening) return;
    issueText = null;
    update({ type: "sent", reference: sent.reference, gh: sent.gh });
    if (sent.gh) loadIssueText();
  } catch (e) {
    answer(asked, { type: "failed", error: String(e) });
  }
}

/// The manifest with what was typed, as the issue says it.
function issueArgs() {
  return { manifest: { ...state.draft!.manifest, text: state.text }, reference: state.reference };
}

async function loadIssueText() {
  const asked = opening;

  try {
    const text = await invoke<string>("report_issue_text", issueArgs());
    if (asked !== opening) return;
    issueText = text;
    if (state.step === "sent") draw();
  } catch (e) {
    answer(asked, { type: "failed", error: String(e) });
  }
}

async function openIssue(b: HTMLButtonElement) {
  b.disabled = true;
  const asked = opening;

  try {
    answer(asked, { type: "issued", url: await invoke<string>("report_issue", issueArgs()) });
  } catch (e) {
    // A failure draws the same tree, so React would not clear this itself.
    b.disabled = false;
    answer(asked, { type: "failed", error: String(e) });
  }
}

/// Back from a failure; on the sent step it asks for the issue text again
/// when that was what failed.
function back() {
  update({ type: "back" });
  if (state.step === "sent" && state.gh && issueText == null) loadIssueText();
}

/// Try again on a failure: the step that failed, once more.
function retry(button: HTMLButtonElement) {
  if (state.reference && issueText == null) return back();
  if (state.reference) return void openIssue(button);
  if (state.draft) return void send();
  collect();
}

function draw() {
  if (!sheet || !render) return;
  const box = sheet, show = render;
  const title = state.step === "sent" || state.step === "issue" ? "Sent" : "Report a problem";
  flushSync(() => show(<ReportBody
    state={state} title={title} opened={opened} issueText={issueText} debug={!!debug}
    update={update} collect={collect} send={send} back={back} retry={retry} openIssue={openIssue}
    cancel={() => { if (!busy()) close?.(); }}
    done={() => close?.()}
    startDebug={() => { const typed = state.text; const start = debug; close?.(); start?.(typed); }}
    openFile={(path: string) => { opened = opened === path ? null : path; draw(); }}
    openUrl={() => { void invoke("open_url", { url: state.issueUrl }).catch(e => console.warn("opening the issue:", e)); }}
  />, title));
  const first = box.querySelector<HTMLElement>("textarea, .rp-foot .pri");
  if (first && !box.contains(document.activeElement)) first.focus();
}
