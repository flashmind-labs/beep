// A machine's traffic (ListMachineTraffic): lifetime bytes by key, such as
// `alpn.beep-sync/1.tx`, read into what the Machines screens show.
import { bytes } from "../center/home-state.ts";

export interface MachineTraffic {
  id: string;
  counters: { key: string; bytes: number }[];
}

export interface TrafficRow {
  label: string;
  sent: number;
  received: number;
}

/// iroh-blobs' ALPN carries sync's file contents along with sent files.
const LABELS: Record<string, string> = {
  "beep-node/1": "Link",
  "beep-sync/1": "File sync",
  "/iroh-bytes/4": "Transfers",
};

/// One row per protocol, the ones without a name by their ALPN last.
export function trafficRows(t: MachineTraffic | undefined): TrafficRow[] {
  const by = new Map<string, TrafficRow>();
  for (const { key, bytes: n } of t?.counters ?? []) {
    const m = /^alpn\.(.+)\.(tx|rx)$/.exec(key);
    if (!m) continue;
    let row = by.get(m[1]);
    if (!row) by.set(m[1], row = { label: LABELS[m[1]] ?? m[1], sent: 0, received: 0 });
    if (m[2] === "tx") row.sent = n; else row.received = n;
  }
  const order = Object.values(LABELS);
  const rank = (r: TrafficRow) => (order.indexOf(r.label) + order.length + 1) % (order.length + 1);
  return [...by.values()].sort((a, b) => rank(a) - rank(b) || a.label.localeCompare(b.label));
}

/// "1.2 GB sent, 300 MB received" through a relay; null when none was.
export function relayLine(t: MachineTraffic | undefined) {
  const of = (key: string) => t?.counters.find(c => c.key === key)?.bytes ?? 0;
  const sent = of("endpoint.relay.tx"), received = of("endpoint.relay.rx");
  return sent || received ? `${bytes(sent)} sent, ${bytes(received)} received` : null;
}

/// The Machines table's sub-row: each protocol's bytes over the totals.
export function trafficSummary(rows: TrafficRow[]) {
  const sum = (of: (r: TrafficRow) => number) => rows.reduce((n, r) => n + of(r), 0);
  return {
    each: rows.map(r => `${r.label} ${bytes(r.sent + r.received)}`).join(" · "),
    total: `${bytes(sum(r => r.sent))} sent · ${bytes(sum(r => r.received))} received`,
  };
}
