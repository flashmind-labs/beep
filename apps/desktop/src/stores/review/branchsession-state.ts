// Which session works on a branch.

interface Session {
  machine_id: string;
  cwd: string;
  last_activity: number;
  /// Where the machine reports the session working, when known.
  worktree?: string;
}

const normalize = (path: string) => {
  const folder = path.replace(/\\/g, "/").replace(/\/+$/, "") || "/";
  return /^[a-z]:(\/|$)/i.test(folder) ? folder.toLowerCase() : folder;
};

/// Sessions in this checkout, newest first. A nested worktree owns its
/// sessions even when it lives inside the main checkout.
export function branchSessions<S extends Session>(sessions: readonly S[], machineId: string | undefined, worktree: string | null | undefined, worktrees: readonly (string | null)[] = []) {
  if (!machineId || !worktree) return [];
  const folder = normalize(worktree);
  const within = (cwd: string, root: string) => cwd === root || cwd.startsWith(root === "/" ? root : `${root}/`);
  const nested = worktrees.filter((p): p is string => !!p).map(normalize).filter(p => p !== folder && within(p, folder));
  return sessions
    .filter(s => {
      if (s.machine_id !== machineId || !(s.worktree || s.cwd)) return false;
      const cwd = normalize(s.worktree || s.cwd);
      return within(cwd, folder) && !nested.some(root => within(cwd, root));
    })
    .sort((a, b) => b.last_activity - a.last_activity);
}
