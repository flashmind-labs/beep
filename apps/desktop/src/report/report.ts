// report/: the "Report a problem" sheet. Main file: this one; its logic is
// in report-state.ts.
import { invoke } from "../ui/tauri.ts";
import { button, el, present } from "../ui/dom.ts";
import { copyButton, openSheet } from "../ui/parts.ts";
import { PERIODS, START, answerLabel, canSend, next, sizeLabel, type Draft, type ReportEvent, type ReportState, type Sent } from "./report-state.ts";

let state = START, sheet: HTMLDivElement | null = null, close: (() => void) | null = null;
/// The file whose text shows under the list, and the issue body before Yes.
let opened: string | null = null, issueText: string | null = null;

/// Opens the sheet over `host`; a second call while it is open does nothing.
export function openReport(host: HTMLElement) {
  if (sheet) return;
  state = START;
  opened = issueText = null;
  let root: HTMLDivElement;
  ({ root, sheet, close } = openSheet("Report a problem", "am-sheet ip-sheet", { canClose: () => !busy(), onClose: closed }));
  host.append(root);
  draw();
  sheet.querySelector("textarea")?.focus();
}

function closed() {
  sheet = close = null;
}

function busy() {
  return state.step === "collecting" || state.step === "sending";
}

function update(event: ReportEvent) {
  state = next(state, event);
  if (sheet) draw();
}

async function collect() {
  update({ type: "collect" });
  if (state.step !== "collecting") return;
  try {
    const draft = await invoke<Draft>("report_collect", { period: state.period });
    update({ type: "collected", draft });
  } catch (e) {
    update({ type: "failed", error: String(e) });
  }
}

async function send() {
  const draft = state.draft;
  update({ type: "send" });
  if (!draft || state.step !== "sending") return;
  try {
    const sent = await invoke<Sent>("report_send", { draft, text: state.text });
    issueText = null;
    update({ type: "sent", reference: sent.reference, gh: sent.gh });
    if (sent.gh) loadIssueText();
  } catch (e) {
    update({ type: "failed", error: String(e) });
  }
}

/// The manifest with what was typed, as the issue says it.
function issueArgs() {
  return { manifest: { ...state.draft!.manifest, text: state.text }, reference: state.reference };
}

async function loadIssueText() {
  try {
    issueText = await invoke<string>("report_issue_text", issueArgs());
    if (sheet && state.step === "sent") draw();
  } catch (e) {
    update({ type: "failed", error: String(e) });
  }
}

async function openIssue(b: HTMLButtonElement) {
  b.disabled = true;
  try {
    update({ type: "issued", url: await invoke<string>("report_issue", issueArgs()) });
  } catch (e) {
    update({ type: "failed", error: String(e) });
  }
}

/// Try again on a failure: the step that failed, once more.
function retry(e: MouseEvent) {
  if (state.reference && issueText == null) {
    update({ type: "back" });
    return void loadIssueText();
  }
  if (state.reference) return void openIssue(e.currentTarget as HTMLButtonElement);
  if (state.draft) return void send();
  collect();
}

function draw() {
  const box = sheet!;
  const title = state.step === "sent" || state.step === "issue" ? "Sent" : "Report a problem";
  box.setAttribute("aria-label", title);
  box.replaceChildren(el("div", "am-head", el("b", "", title), el("kbd", "", "esc")), ...body());
  const first = box.querySelector<HTMLElement>("textarea, .rp-foot .pri");
  if (first && !box.contains(document.activeElement)) first.focus();
}

function foot(...kids: (HTMLElement | null)[]) {
  return el("div", "rp-foot", ...kids);
}

function cancel() {
  return button("Cancel", "dbtn quiet", () => close?.());
}

function body(): HTMLElement[] {
  switch (state.step) {
    case "form":
    case "collecting":
      return form();
    case "files":
    case "sending":
      return files();
    case "sent":
      return sent();
    case "issue":
      return [
        refRow(),
        el("div", "rp-row rp-link", el("span", "m", "Issue"), el("code", "", state.issueUrl ?? "")),
        foot(button("Open", "dbtn", () => invoke("open_url", { url: state.issueUrl }).catch(e => console.warn("opening the issue:", e))), button("Done", "dbtn pri", () => close?.())),
      ];
    case "failed": {
      const err = el("div", "rp-err", state.error ?? "");
      err.setAttribute("role", "alert");
      return [err, foot(button("Back", "dbtn quiet", () => update({ type: "back" })), button("Try again", "dbtn pri", retry))];
    }
  }
}

function form() {
  const collecting = state.step === "collecting";
  const text = el("textarea", "rp-ta");
  text.placeholder = "What happened?";
  text.setAttribute("aria-label", "What happened?");
  text.value = state.text;
  text.disabled = collecting;
  text.addEventListener("input", () => { state = next(state, { type: "text", text: text.value }); });
  const chips = el("div", "dchips rp-periods", ...PERIODS.map(p => {
    const b = button(p.label, "dchip", () => update({ type: "period", period: p.period }));
    b.setAttribute("role", "radio");
    b.setAttribute("aria-checked", String(state.period === p.period));
    b.setAttribute("aria-pressed", String(state.period === p.period));
    b.disabled = collecting;
    return b;
  }));
  chips.setAttribute("role", "radiogroup");
  chips.setAttribute("aria-label", "Period");
  const go = button("Collect", "dbtn pri", collect);
  go.disabled = collecting;
  return present([
    el("div", "am-label", "What happened?"),
    text,
    chips,
    collecting ? el("div", "am-wait rp-wait", el("span", "dspin"), "Collecting from your machines") : null,
    foot(cancel(), go),
  ]);
}

function files() {
  const draft = state.draft!, sending = state.step === "sending";
  const machines = draft.manifest.machines.map(m => {
    const said = answerLabel(m.answer);
    const bad = m.answer === "Silent" || (typeof m.answer === "object" && "Failed" in m.answer);
    return el("div", "rp-row", el("span", "", m.name), el("span", bad ? "m bad" : "m", said));
  });
  const bytes = new Map(draft.manifest.files.map(f => [f.path, f.bytes]));
  const rows = draft.files.map(f => {
    const b = button(el("code", "", f.path), "rp-row", () => {
      opened = opened === f.path ? null : f.path;
      draw();
    });
    b.append(el("span", "m", sizeLabel(bytes.get(f.path) ?? f.text.length)));
    b.setAttribute("aria-current", String(opened === f.path));
    b.setAttribute("aria-expanded", String(opened === f.path));
    return b;
  });
  const shown = draft.files.find(f => f.path === opened);
  const total = draft.manifest.files.reduce((n, f) => n + f.bytes, 0);
  const go = button("Send", "dbtn pri", () => void send());
  go.disabled = !canSend(state);
  return present([
    el("div", "am-label", "Machines"),
    el("div", "rp-box", ...machines),
    el("div", "am-label", "Files"),
    el("div", "rp-box", ...rows),
    shown ? el("div", "rp-pane-head", el("b", "", el("code", "", shown.path)), el("small", "", "Read only, secrets and paths removed")) : null,
    shown ? pane(shown.path, shown.text) : null,
    el("div", "rp-total", el("span", "", "Total"), el("span", "", sizeLabel(total))),
    sending ? el("div", "am-wait rp-wait", el("span", "dspin"), "Sending") : null,
    foot(cancel(), go),
  ]);
}

/// A file's text, read only, with what redaction took out marked.
function pane(path: string, text: string) {
  const pre = el("pre", "rp-pane");
  pre.setAttribute("aria-label", path);
  const parts = text.split(/(\[redacted\])/);
  pre.append(...parts.map((p, i) => (i % 2 ? el("span", "r", p) : p)));
  return pre;
}

function refRow() {
  const ref = state.reference ?? "";
  return el("div", "rp-ref", el("span", "", ref), copyButton(ref, "Copy", "dbtn small"));
}

function sent() {
  if (!state.gh) return [refRow(), foot(button("Done", "dbtn pri", () => close?.()))];
  const issue = el("pre", "rp-issue", issueText ?? "");
  issue.setAttribute("aria-label", "Issue text");
  const yes = button("Yes", "dbtn pri", () => void openIssue(yes));
  yes.disabled = issueText == null;
  return [
    refRow(),
    el("div", "am-label", "Report it on GitHub too?"),
    issueText == null ? el("div", "am-wait rp-wait", el("span", "dspin"), "Writing the issue") : issue,
    foot(button("No", "dbtn quiet", () => close?.()), yes),
  ];
}
