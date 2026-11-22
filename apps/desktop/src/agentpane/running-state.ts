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

export function runningRow(tree: RunningTree) {
  const { root, children } = tree;
  const at = new Date(root.started_at * 1000);
  const time = `${String(at.getHours()).padStart(2, "0")}:${String(at.getMinutes()).padStart(2, "0")}`;
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
