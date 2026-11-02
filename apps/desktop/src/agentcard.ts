// Reads what a `code_agent` call said of its session, for the card a chat
// reply shows in place of a bare "Used a coding agent" step. The server
// writes the result for the model:
//
//   claude session a3f9c1 in /srv/project: working, 2 turns
//   Doing: Running the auth tests
//   Plan:
//     [done] Find why the login test fails
//     [now] Run the auth tests
//   Files changed: src/auth/session.ts, tests/login.test.ts
//   Question 3: Run git push?

/// A plan step: `completed`, `in_progress` or `pending`, as `agent_view`
/// gives them.
export interface ReportStep {
  content: string;
  status: string;
}

/// One session as a `code_agent` result describes it.
export interface AgentReport {
  agent: string;
  id: string;
  cwd: string;
  /// starting, working, asking, idle or ended; "unknown" when the server
  /// lost track of it.
  status: string;
  title: string | null;
  doing: string | null;
  plan: ReportStep[];
  files: string[];
  question: string | null;
}

const HEAD = /^(\S+) session (\S+) in (.+?): (.+), \d+ turns?(?:,.*)?$/;
const MARKS: Record<string, string> = { done: "completed", now: "in_progress", todo: "pending" };

/// The session `text` describes, or null when it describes none (a list
/// of sessions, a stop, a failure).
export function parseAgentReport(text: string | null | undefined): AgentReport | null {
  const lines = String(text ?? "").split("\n");
  const head = HEAD.exec(lines[0]?.trim() ?? "");
  if (!head) return null;
  const [, agent, id, cwd, said] = head;
  const status = said.startsWith("ended") ? "ended" : said.startsWith("lost track") ? "unknown" : said.trim();
  const report: AgentReport = { agent, id, cwd, status, title: null, doing: null, plan: [], files: [], question: null };
  let inPlan = false;
  for (const line of lines.slice(1)) {
    const step = inPlan ? /^\s+\[(done|now|todo)\] (.*)$/.exec(line) : null;
    if (step) { report.plan.push({ content: step[2], status: MARKS[step[1]] }); continue; }
    inPlan = line === "Plan:";
    if (line.startsWith("Title: ")) report.title = line.slice(7);
    else if (line.startsWith("Doing: ")) report.doing = line.slice(7);
    else if (line.startsWith("Files changed: ")) report.files = line.slice(15).split(", ").filter(f => f.trim());
    else if (/^Question \S+: /.test(line)) report.question = line.replace(/^Question \S+: /, "");
    // The agent's last message and what follows are the model's to read.
    else if (line.startsWith("Last turn ")) break;
  }
  return report;
}

/// The card a reply keeps for each session, by session id: a later call
/// about the same session updates its card rather than adding one.
export function cardKey(r: Pick<AgentReport, "agent" | "id">) {
  return `${r.agent}:${r.id}`;
}
