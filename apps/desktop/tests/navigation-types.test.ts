import test from "node:test";
import assert from "node:assert/strict";
import type { go, goTo, registerView } from "../src/center/views.ts";
import { mainTarget } from "../src/project/projectwindow.ts";
import type { BackTo } from "../src/bar/core.ts";

// The compiler checks these contracts without loading the window's DOM modules.
export function navigationContracts(navigate: typeof go, open: typeof goTo, register: typeof registerView) {
  navigate("home");
  navigate("projects", { project: "acme/widget" });
  navigate("researchopen", { session: "research" });
  open({ view: "pane", arg: { agentnew: { prompt: "" } } });
  // @ts-expect-error A research page requires its session.
  navigate("researchopen");
  // @ts-expect-error A project selection cannot be passed to Home.
  navigate("home", { project: "acme/widget" });
  // @ts-expect-error Pane page names are a fixed set.
  navigate("pane", "missing-page");
  // @ts-expect-error Page and argument must describe the same destination.
  open({ view: "researchopen", arg: { project: "acme/widget" } });
  // @ts-expect-error Return destinations require the page's own argument type.
  const invalidReturn: BackTo = { view: "projects", arg: { session: "research" } };
  void invalidReturn;
  register("researchopen", {
    mount(_root, arg) {
      const session: string = arg.session;
      void session;
      // @ts-expect-error Research arguments cannot contain a project selection.
      void arg.project;
    },
  });
}

test("typed locations preserve the native window targets", () => {
  assert.equal(mainTarget({ view: "home", arg: null }), "home");
  assert.deepEqual(mainTarget({ view: "researchopen", arg: { session: "research" } }), { open_research: { session: "research" } });
  assert.deepEqual(mainTarget({ view: "projects", arg: null }), { projects: { project: null, run: null } });
  assert.deepEqual(mainTarget({ view: "projects", arg: { project: "acme/widget", run: { id: 42, host: "forge" } } }), { projects: { project: "acme/widget", run: { id: 42, host: "forge" } } });
  assert.deepEqual(mainTarget({ view: "pane", arg: { chat: { session: null } } }), { chat: { session: null } });
  assert.deepEqual(mainTarget({ view: "pane", arg: { group: { id: "group" } } }), { group: { id: "group" } });
  assert.equal(mainTarget({ view: "pane", arg: "settings" }), "settings");
  assert.equal(mainTarget({ view: "pane", arg: "agents" }), "home");
  assert.equal(mainTarget({ view: "sync", arg: null }), "home");
});
