// What the "Report a problem" sheet shows, as pure functions so they can be
// tested without Tauri. report.ts draws it and talks to the app.

/// How far back the logs go (`Period` in beep-report).
export type Period = "hour" | "today" | "day";

/// How a machine answered (`Answer` in beep-report, externally tagged).
export type Answer = "Answered" | "OlderNode" | "Silent" | { Offline: { reason: string } } | { Failed: { reason: string } };

export interface MachineEntry {
  id: string;
  name: string;
  kind: string;
  answer: Answer;
  version: string;
  os: string;
  os_version: string;
}

export interface FileEntry {
  path: string;
  bytes: number;
}

export interface Manifest {
  version: number;
  sender: string;
  mesh: string | null;
  app: string;
  app_version: string;
  text: string;
  period: Period;
  since: number;
  made_at: number;
  machines: MachineEntry[];
  files: FileEntry[];
}

/// One collected file, its text already redacted.
export interface DraftFile {
  path: string;
  text: string;
}

/// What `report_collect` gives: what would be sent.
export interface Draft {
  manifest: Manifest;
  files: DraftFile[];
}

/// What `report_send` gives back.
export interface Sent {
  reference: string;
  gh: boolean;
}

export type Step = "form" | "collecting" | "files" | "sending" | "sent" | "issue" | "failed";

export type ReportState = {
  step: Step;
  text: string;
  period: Period;
  draft?: Draft;
  reference?: string;
  gh?: boolean;
  issueUrl?: string;
  error?: string;
};

export type ReportEvent =
  | { type: "text"; text: string }
  | { type: "period"; period: Period }
  | { type: "collect" }
  | { type: "collected"; draft: Draft }
  | { type: "send" }
  | { type: "sent"; reference: string; gh: boolean }
  | { type: "issued"; url: string }
  | { type: "failed"; error: string }
  | { type: "back" };

/// The period chips, in order.
export const PERIODS: readonly { period: Period; label: string }[] = [
  { period: "hour", label: "Last hour" },
  { period: "today", label: "Today" },
  { period: "day", label: "Last day" },
];

export const START: ReportState = { step: "form", text: "", period: "today" };

/// Whether Send can go: on the files step with something to send.
export function canSend(state: ReportState) {
  return state.step === "files" && !!state.draft && state.draft.files.length > 0;
}

/// The next state after `event`. `back` leaves a failure for the step it
/// came from: the sent step once a reference is in, else the files, else the form.
export function next(state: ReportState, event: ReportEvent): ReportState {
  switch (event.type) {
    case "text":
      return { ...state, text: event.text };
    case "period":
      return state.step === "form" ? { ...state, period: event.period } : state;
    case "collect":
      return state.step === "form" || state.step === "failed" ? { ...state, step: "collecting", error: undefined } : state;
    case "collected":
      return state.step === "collecting" ? { ...state, step: "files", draft: event.draft } : state;
    case "send":
      return canSend(state) || (state.step === "failed" && !!state.draft && !state.reference) ? { ...state, step: "sending", error: undefined } : state;
    case "sent":
      return state.step === "sending" ? { ...state, step: "sent", reference: event.reference, gh: event.gh } : state;
    case "issued":
      return state.reference ? { ...state, step: "issue", issueUrl: event.url, error: undefined } : state;
    case "failed":
      return { ...state, step: "failed", error: event.error };
    case "back":
      if (state.step !== "failed") return state;
      return { ...state, step: state.reference ? "sent" : state.draft ? "files" : "form", error: undefined };
  }
}

/// The line under a failure when the issue timed out: it may have opened anyway.
export function issueHint(state: ReportState) {
  return state.step === "failed" && !!state.reference && /did not finish in time/.test(state.error ?? "")
    ? "Check GitHub before trying again: the issue may have opened."
    : null;
}

/// How a machine's row reads its answer.
export function answerLabel(answer: Answer) {
  if (answer === "Answered") return "answered";
  if (answer === "OlderNode") return "older node";
  if (answer === "Silent") return "did not answer";
  if ("Offline" in answer) return "offline";
  return `failed: ${answer.Failed.reason}`;
}

/// "512 B", "412 KB", "1.2 MB": a file's size, in units of 1024.
export function sizeLabel(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  const mb = bytes / (1024 * 1024);
  return `${mb < 10 ? mb.toFixed(1) : Math.round(mb)} MB`;
}
