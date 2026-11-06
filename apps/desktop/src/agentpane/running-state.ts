// Rows of the session's Running tab. Times are Unix seconds from its machine.
export interface RunningProcess {
  pid: number;
  command: string;
  started_at: number;
  ports: number[];
  left_group: boolean;
}

export function runningRow(process: RunningProcess, local: boolean) {
  const at = new Date(process.started_at * 1000);
  const time = `${String(at.getHours()).padStart(2, "0")}:${String(at.getMinutes()).padStart(2, "0")}`;
  return {
    command: process.command,
    detail: `since ${time}${process.left_group ? " · left the group" : ""}`,
    ports: process.ports.filter(p => Number.isInteger(p) && p > 0 && p <= 65535)
      .map(port => ({ label: `:${port}`, url: local ? `http://localhost:${port}` : null })),
  };
}
