import assert from "node:assert/strict";
import { test } from "node:test";
import { relayLine, trafficRows, trafficSummary } from "../src/machines/traffic-state.ts";

const machine = {
  id: "m",
  counters: [
    { key: "alpn./iroh-bytes/4.rx", bytes: 3_100_000_000 },
    { key: "alpn./iroh-bytes/4.tx", bytes: 14_000_000_000 },
    { key: "alpn.beep-node/1.rx", bytes: 96_000_000 },
    { key: "alpn.beep-node/1.tx", bytes: 412_000_000 },
    { key: "alpn.other/2.tx", bytes: 5 },
    { key: "endpoint.relay.rx", bytes: 300_000_000 },
    { key: "endpoint.relay.tx", bytes: 1_200_000_000 },
  ],
};

test("rows name the protocols, known ones first", () => {
  assert.deepEqual(trafficRows(machine), [
    { label: "Link", sent: 412_000_000, received: 96_000_000 },
    { label: "Transfers", sent: 14_000_000_000, received: 3_100_000_000 },
    { label: "other/2", sent: 5, received: 0 },
  ]);
  assert.deepEqual(trafficRows(undefined), []);
});

test("the summary adds each protocol up and totals each way", () => {
  const { each, total } = trafficSummary(trafficRows(machine));
  assert.equal(each, "Link 508 MB · Transfers 17 GB · other/2 5 B");
  assert.equal(total, "14 GB sent · 3.2 GB received");
});

test("the relay line shows only when a relay carried bytes", () => {
  assert.equal(relayLine(machine), "1.2 GB sent, 300 MB received");
  assert.equal(relayLine({ id: "m", counters: [] }), null);
});
