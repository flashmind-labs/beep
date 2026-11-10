const forgotten = new Set<string>();
const PREFIX = "beep.projectView.";
export function viewKey(machine: string, session: string) { return `${machine}\n${session}`; }
export function readView(key: string): unknown {
  try { return JSON.parse(localStorage.getItem(PREFIX + key) ?? "null"); } catch { return null; }
}
export function writeView(key: string, value: unknown) {
  if ([...forgotten].some(session => (key === session || key.startsWith(session + ":")))) return;
  try { localStorage.setItem(PREFIX + key, JSON.stringify(value)); } catch { /* Storage may be full. */ }
}
export function forgetView(machine: string, session: string) {
  const key = viewKey(machine, session);
  forgotten.add(key);
  try {
    for (const name of Object.keys(localStorage)) if ((name === PREFIX + key || name.startsWith(PREFIX + key + ":"))) localStorage.removeItem(name);
  } catch { /* Storage may be unavailable. */ }
}

