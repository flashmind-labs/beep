import { answers, calls } from "./bar-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { invoke } from "../src/ui/tauri.ts";

test("agent starts log the request and returned session without logging their contents", async () => {
  calls.length = 0;
  answers.agent_start = { session: { id: "s1", machine_id: "m1", status: "starting", cwd: "/srv/project", title: "Private task" } };
  await invoke("agent_start", { machineId: "m1", prompt: "Private prompt", cwd: "/srv/project", images: [{ data: "private-image" }] });
  const logs = calls.filter(c => c.cmd === "log_webview").map(c => c.args as { message: string; info: boolean });
  assert.equal(logs.length, 2);
  assert.ok(logs.every(l => l.info));
  assert.match(logs[0].message, /"stage":"requested"/);
  assert.match(logs[1].message, /"stage":"returned"/);
  assert.match(logs[1].message, /"session":"s1"/);
  assert.ok(logs.every(l => !/Private|private-image|\/srv\/project/.test(l.message)));
});
