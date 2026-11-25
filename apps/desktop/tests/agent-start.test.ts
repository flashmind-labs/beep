import { answers, bar, calls } from "./chat-dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { AG, type AgentHost } from "../src/agents/agents-state.ts";
import { keepProjectAccount } from "../src/agents/agentmodels.ts";
import { renderAgentNew, startAgent } from "../src/agents/agentnew.tsx";
import "../src/bar/bar-app.tsx";

const settle = () => new Promise(resolve => setImmediate(resolve));
const starts = () => calls.filter(call => call.cmd === "agent_begin");

function deferred<T>() {
  let resolve = (_value: T) => {}, reject = (_error: Error) => {};
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}

async function attach(name: string, path: string) {
  const field = bar.querySelector<HTMLInputElement>('input[type="file"]');
  assert.ok(field);
  const sent = deferred<void>();
  Object.defineProperty(answers, "agent_tree_upload", { configurable: true, get() { sent.resolve(); return path; } });
  Object.defineProperty(field, "files", { value: [new window.File(["Fixture"], name)] });
  field.dispatchEvent(new window.Event("change", { bubbles: true }));
  await sent.promise;
  await settle();
  delete answers.agent_tree_upload;
}

test("a submitted form keeps its account and cannot finish a later form's start", { timeout: 5000 }, async t => {
  t.mock.method(window.HTMLCanvasElement.prototype, "getContext", () => null);
  Object.defineProperty(globalThis, "FileReader", { configurable: true, value: window.FileReader });
  t.after(() => { Reflect.deleteProperty(globalThis, "FileReader"); });
  const host: AgentHost = {
    machine_id: "box", machine_name: "Build box", agents: ["codex"], workspace: "/srv/project",
    home: null, capabilities: [], assistant: false, moves_chats: false,
  };
  const accounts = [{ agent: "codex", name: "account-a" }, { agent: "codex", name: "account-b" }];
  AG.hosts = answers.agent_hosts = [host];
  answers.agent_sessions = [];
  answers.agent_account_names = accounts;
  keepProjectAccount("project-a", "codex", "account-a");
  keepProjectAccount("project-b", "codex", "account-b");
  renderAgentNew("First prompt", "hide", { machine: "box", project: "project-a", cwd: "/srv/project/a" });
  await attach("first.txt", ".beep/uploads/first.txt");

  const names = deferred<typeof accounts>();
  const first = deferred<never>();
  void first.promise.catch(() => {});
  t.after(() => { names.resolve(accounts); first.reject(new Error("Test ended")); });
  const validatingAccount = deferred<void>();
  Object.defineProperty(answers, "agent_account_names", {
    configurable: true, get() { validatingAccount.resolve(); return names.promise; },
  });
  answers.agent_begin = first.promise;
  const startingFirst = startAgent();
  await validatingAccount.promise;
  assert.equal(starts().length, 0, "the first start waits for account validation");

  renderAgentNew("Second prompt", "hide", { machine: "box", project: "project-b", cwd: "/srv/project/b" });
  await attach("second.txt", ".beep/uploads/second.txt");
  names.resolve(accounts);
  await settle();
  const request = starts()[0]?.args;
  assert.ok(request && typeof request === "object");
  assert.equal(Reflect.get(request, "prompt"), "First prompt\n\n@.beep/uploads/first.txt");
  assert.equal(Reflect.get(request, "account"), "account-a");
  assert.equal(Reflect.get(request, "cwd"), "/srv/project/a");

  const second = deferred<never>();
  void second.promise.catch(() => {});
  t.after(() => { second.reject(new Error("Test ended")); });
  answers.agent_begin = second.promise;
  const startingSecond = startAgent();
  await settle();
  assert.equal(starts().length, 2);
  const anotherRequest = starts()[1]?.args;
  assert.ok(anotherRequest && typeof anotherRequest === "object");
  assert.equal(Reflect.get(anotherRequest, "prompt"), "Second prompt\n\n@.beep/uploads/second.txt");
  first.reject(new Error("First start failed"));
  await startingFirst;
  assert.equal(bar.querySelector("#aggo")?.hasAttribute("disabled"), true);
  assert.doesNotMatch(bar.textContent ?? "", /First start failed/);
  assert.equal(bar.querySelector<HTMLTextAreaElement>("#agprompt")?.value, "Second prompt");
  assert.equal(bar.querySelector(".cc-pjc .attachment-count")?.textContent, "1 file");

  second.reject(new Error("Second start failed"));
  await startingSecond;
  assert.equal(bar.querySelector("#aggo")?.hasAttribute("disabled"), false);
  assert.match(bar.textContent ?? "", /Second start failed/);
});
