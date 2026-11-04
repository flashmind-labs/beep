// Machines the account lists that are not in the mesh yet, and what their rows say. Each joins
// once by hand; until then it still works through the account.

/// The machines in the mesh, and those only the account lists.
export function splitJoined<T extends { joined?: boolean }>(machines: readonly T[]) {
  return {
    mesh: machines.filter(m => m.joined !== false),
    outside: machines.filter(m => m.joined === false),
  };
}

/// The line under a machine that is not in the mesh yet.
export function notJoinedSub(os: string) {
  return `${os ? `${os} · ` : ""}still works through your account`;
}

/// What the Add a machine sheet and a not-joined row put on the clipboard: the line a node runs.
export function joinLine(ticket: string) {
  return `beep-node mesh join ${ticket}`;
}
