import { answers, calls } from "./bar-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { invoke } from "../src/ui/tauri.ts";

test("agent starts log the request and returned session without logging their contents", async () => {
  calls.length = 0;
  answers.agent_start = { session: { id: "s1", machine_id: "m1", status: "starting", cwd: "/srv/project", title: "Private task" } };
  await invoke("agent_start", { machineId: "m1", prompt: "Private prompt", cwd: "/srv/project", images: [{ data: "private-image" }] });
  const logs = calls.filter(c => c.cmd === "log_webview").map(c => c.args as { message: string; info: boolean }).filter(l => l.message.startsWith("agent command:"));
  assert.equal(logs.length, 2);
  assert.ok(logs.every(l => l.info));
  assert.match(logs[0].message, /"stage":"requested"/);
  assert.match(logs[1].message, /"stage":"returned"/);
  assert.match(logs[1].message, /"session":"s1"/);
  assert.ok(logs.every(l => !/Private|private-image|\/srv\/project/.test(l.message)));
});

test("ordinary commands trace receipt and completion with one ID and no arguments", async () => {
  calls.length = 0;
  answers.agent_tree = { entries: [] };
  await invoke("agent_tree", { root: "/srv/private", prompt: "private text", token: "private token" });
  const logs = calls.filter(call => call.cmd === "log_webview")
    .map(call => (call.args as { message: string }).message)
    .filter(message => message.startsWith("command trace:"));
  assert.equal(logs.length, 2);
  const samples = logs.map(message => JSON.parse(message.slice("command trace: ".length)) as { request: number; stage: string; outcome: string });
  assert.equal(samples[0].request, samples[1].request);
  assert.deepEqual(samples.map(sample => sample.stage), ["requested", "returned"]);
  assert.equal(samples[1].outcome, "ok");
  assert.ok(logs.every(message => !message.includes("private")));
});
