import { answers, calls, nativeEvent } from "./bar-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { P, type MediaTab } from "../src/stores/agentpane/pane-state.ts";
import { paneModel } from "../src/parts/session/media-layout.ts";
import { loadText } from "../src/parts/session/read.ts";

const setTabs = (tabs: MediaTab[]) => paneModel.setState({ tabs });


const reads = () => calls.filter(c => c.cmd === "agent_tree_text");

test("a text tab whose read lost the link reads its file again on reconnect, in the same place", async () => {
  P.session = { id: "s1", machine_id: "m1", machine_name: "build-box", cwd: "/srv/project", agent: "claude" };
  const tab: MediaTab = {
    id: "/srv/project/a.txt", kind: "text", name: "a.txt", path: null, url: null, size: 0, started: 0, done: 0, error: "",
    rev: 0, viewHidden: false, note: null, player: null, playerAt: 0, rel: "a.txt",
    elsewhere: [{ root: "/srv", rel: "project/a.txt" }],
  };
  setTabs([tab]);
  answers.agent_tree_text = Promise.reject(new Error("connection lost"));
  (answers.agent_tree_text as Promise<unknown>).catch(() => {});

  await loadText(tab);
  assert.equal(tab.error, "connection lost");
  assert.equal(tab.rel, "a.txt", "a dropped link is no reason to look elsewhere");

  answers.agent_tree_text = { text: "hello", size: 5, binary: false, truncated: false };
  const before = reads().length;
  nativeEvent("reconnected");
  await new Promise(resolve => setImmediate(resolve));

  assert.equal(reads().length, before + 1);
  assert.equal(tab.error, "");
  assert.equal(tab.text?.text, "hello");
  assert.equal(tab.tries, 0);
  setTabs([]);
});

test("a text tab whose file is not there looks in the next place and does not retry", async () => {
  P.session = { id: "s1", machine_id: "m1", machine_name: "build-box", cwd: "/srv/project", agent: "claude" };
  const tab: MediaTab = {
    id: "/srv/project/b.txt", kind: "text", name: "b.txt", path: null, url: null, size: 0, started: 0, done: 0, error: "",
    rev: 0, viewHidden: false, note: null, player: null, playerAt: 0, rel: "b.txt",
    elsewhere: [{ root: "/srv", rel: "project/b.txt" }],
  };
  setTabs([tab]);
  answers.agent_tree_text = Promise.reject(new Error("b.txt is not there"));
  (answers.agent_tree_text as Promise<unknown>).catch(() => {});

  await loadText(tab);
  assert.equal(tab.rel, "project/b.txt");
  assert.equal(tab.tries, undefined);
  setTabs([]);
});
