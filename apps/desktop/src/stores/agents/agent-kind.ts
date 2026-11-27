// How each coding agent is named and tinted, and a folder named by its last part.
export interface AgentKind {
  ini: string;
  name: string;
  cls: string;
}
const AGENT_KINDS: Record<string, AgentKind> = {
  claude: { ini: "CC", name: "Claude Code", cls: "claude" },
  codex: { ini: "CX", name: "Codex", cls: "codex" },
  agy: { ini: "AG", name: "Antigravity", cls: "agy" },
  pi: { ini: "PI", name: "pi", cls: "pi" },
  flsh: { ini: "FL", name: "flsh", cls: "flsh" },
  opencode: { ini: "OC", name: "OpenCode", cls: "opencode" },
  hermes: { ini: "HE", name: "Hermes", cls: "hermes" },
};

export const agentKind = (a: string): AgentKind => AGENT_KINDS[a] ?? { ini: String(a || "?").slice(0, 2).toUpperCase(), name: a || "Agent", cls: "" };
export const folderName = (p: string) => String(p || "").split("/").filter(Boolean).pop() || p || "";
