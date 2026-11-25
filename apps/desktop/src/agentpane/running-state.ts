// Rows of the session's Running tab. Times are Unix seconds from its machine.
export interface RunningProcess {
  pid: number;
  command: string;
  started_at: number;
  ports: number[];
  left_group: boolean;
  /// The process that started it; 0 from nodes that do not say.
  parent: number;
  /// The ports that answer with a page or a redirect; empty from nodes that do not say.
  pages: number[];
}

/// A Docker container the session's agent started. `id` is Docker's.
export interface RunningContainer {
  id: string;
  name: string;
  image: string;
  started_at: number;
  ports: number[];
  /// The ports that answer with a page or a redirect.
  pages: number[];
}

/// What a session's `processes.<id>` topic pushed: its whole list, or why the node could not make it.
export type ProcessesValue =
  | { kind: "list"; processes: RunningProcess[]; containers: RunningContainer[] }
  | { kind: "failed"; message: string };

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null && !Array.isArray(value);
const numbers = (value: unknown): value is number[] => Array.isArray(value) && value.every(n => typeof n === "number");

function isProcess(value: unknown): value is RunningProcess {
  return isRecord(value) && typeof value.pid === "number" && typeof value.command === "string" && typeof value.started_at === "number"
    && numbers(value.ports) && typeof value.left_group === "boolean" && typeof value.parent === "number" && numbers(value.pages);
}

function isContainer(value: unknown): value is RunningContainer {
  return isRecord(value) && typeof value.id === "string" && typeof value.name === "string" && typeof value.image === "string"
    && typeof value.started_at === "number" && numbers(value.ports) && numbers(value.pages);
}

/// Reads the update's JSON, which is the node's `events::Value` as serde writes it: the unit
/// `"Missing"` when the session has no state, else `{ Sessions: { Processes: { processes, containers } } }`
/// (no `containers` from older nodes)
/// or `{ Sessions: { Error: { message } } }`; native code sends `{ error }` when the node's read failed.
/// Null for anything else.
export function parseProcesses(data: unknown): ProcessesValue | null {
  if (data === "Missing") return { kind: "list", processes: [], containers: [] };
  if (isRecord(data) && typeof data.error === "string") return { kind: "failed", message: data.error };
  const reply = isRecord(data) && isRecord(data.Sessions) ? data.Sessions : null;
  if (!reply) return null;

  const listed = isRecord(reply.Processes) ? reply.Processes.processes : null;
  const containers = isRecord(reply.Processes) ? reply.Processes.containers ?? [] : null;
  if (Array.isArray(listed) && listed.every(isProcess) && Array.isArray(containers) && containers.every(isContainer)) {
    return { kind: "list", processes: listed, containers };
  }

  return isRecord(reply.Error) && typeof reply.Error.message === "string" ? { kind: "failed", message: reply.Error.message } : null;
}

/// A row: a command the session started and the processes under it.
export interface RunningTree { root: RunningProcess; children: RunningProcess[] }

/// Groups `processes` under the furthest ancestor in the list, in list order.
export function runningTrees(processes: readonly RunningProcess[]): RunningTree[] {
  const byPid = new Map(processes.map(p => [p.pid, p]));
  const trees = new Map<number, RunningTree>();

  for (const process of processes) {
    let root = process;
    for (let up = byPid.get(root.parent), hops = 0; up && up !== root && hops < processes.length; up = byPid.get(root.parent), hops++) root = up;
    const tree = trees.get(root.pid) ?? { root, children: [] };
    trees.set(root.pid, tree);
    if (process !== root) tree.children.push(process);
  }

  return [...trees.values()];
}

/// Unix seconds `at` as the local hour and minute.
function clock(at: number) {
  const date = new Date(at * 1000);
  return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

export function runningRow(tree: RunningTree) {
  const { root, children } = tree;
  const time = clock(root.started_at);
  const more = children.length ? ` · ${children.length} more process${children.length === 1 ? "" : "es"}` : "";
  const ports = [...new Set([root, ...children].flatMap(p => p.ports))].filter(p => Number.isInteger(p) && p > 0 && p <= 65535);
  const page = pagePort([root, ...children], ports);

  return {
    command: shownCommand(root.command),
    detail: `since ${time}${more}${root.left_group ? " · left the group" : ""}`,
    page: page === null ? null : { label: `:${page}`, port: page },
    /// With a page, the other ports, which show only with the whole command.
    ports: ports.filter(port => port !== page).map(port => ({ label: `:${port}`, port })),
  };
}

/// A container's row: its name, image and start time, and the page it shows.
export function containerRow(container: RunningContainer) {
  const ports = container.ports.filter(p => Number.isInteger(p) && p > 0 && p <= 65535);
  const page = container.pages.find(port => ports.includes(port)) ?? null;

  return {
    name: container.name || container.id.slice(0, 12),
    detail: `${container.image} · since ${clock(container.started_at)}`,
    page: page === null ? null : { label: `:${page}`, port: page },
    ports: ports.filter(port => port !== page).map(port => ({ label: `:${port}`, port })),
  };
}

/// A port a command names: `--port 8788`, `-p 8788`, `PORT=8788` or `localhost:8788`.
const NAMED = /(?:--port[= ]|-p ?|\bPORT=|:)(\d{2,5})\b/g;

/// The port a row's server shows its page on: among those that answered with a page, the one
/// its command names, else the first.
function pagePort(processes: readonly RunningProcess[], ports: readonly number[]) {
  const pages = processes.flatMap(p => p.pages).filter(port => ports.includes(port));
  const named = processes.flatMap(p => [...p.command.matchAll(NAMED)].map(match => Number(match[1])));

  return pages.find(port => named.includes(port)) ?? pages[0] ?? null;
}

/// Claude Code runs a command as `zsh -c source <snapshot> && … && eval '<command>' …`.
const EVAL = /\beval '((?:[^']|'\\'')*)'?/;
/// `sh -c`, `bash -lc` and the like.
const SHELL = /^(?:\S*[/\\])?(?:ba|z|da|fi|k)?sh(?:\.exe)?\s+-\w*c\s+/;
const PATH = /(?:\S*[/\\]node_modules[/\\](?:\.bin[/\\])?|(?:\/home|\/Users)\/[^/\s]+\/)/g;

/// The command an agent ran, out of its shell wrapper, with shorter paths.
export function shownCommand(command: string) {
  const ran = command.match(EVAL)?.[1]?.replaceAll("'\\''", "'") ?? command.replace(SHELL, "");
  const [program = "", ...args] = ran.trim().split(" ");
  const name = program.split(/[/\\]/).pop() ?? program;

  return [name, ...args].join(" ").replace(PATH, path => path.includes("node_modules") ? "" : "~/");
}
