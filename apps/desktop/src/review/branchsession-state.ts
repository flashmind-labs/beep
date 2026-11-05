// Which session works on a branch.

interface Session {
  machine_id: string;
  cwd: string;
  last_activity: number;
}

/// The most recently active session in `worktree`, the folder that has the
/// branch checked out now, on `machineId`; undefined when there is none. A session on a
/// shared checkout that moved to another branch is not in the branch's worktree.
export function branchSession<S extends Session>(sessions: readonly S[], machineId: string | undefined, worktree: string | null | undefined) {
  if (!machineId || !worktree) return undefined;
  const folder = worktree.replace(/\/+$/, "");
  return sessions
    .filter(s => s.machine_id === machineId && s.cwd?.replace(/\/+$/, "") === folder)
    .sort((a, b) => b.last_activity - a.last_activity)[0];
}
