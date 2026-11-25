import test from "node:test";
import assert from "node:assert/strict";
import type { go, goTo } from "../src/center/nav.ts";
import type { CenterView } from "../src/center/state.ts";
import { mainTarget } from "../src/project/projectwindow.ts";
import type { BackTo } from "../src/bar/core.ts";

// The compiler checks these contracts without loading the window's DOM modules.
export function navigationContracts(navigate: typeof go, open: typeof goTo) {
  navigate("home");
  navigate("projects", { project: "acme/widget" });
  navigate("instructions", { tab: "history" });
  open({ view: "pane", arg: { agentnew: { prompt: "", assistant: false } } });
  // @ts-expect-error A project selection cannot be passed to Home.
  navigate("home", { project: "acme/widget" });
  // @ts-expect-error Pane page names are a fixed set.
  navigate("pane", "missing-page");
  // @ts-expect-error Page and argument must describe the same destination.
  open({ view: "instructions", arg: { project: "acme/widget" } });
  // @ts-expect-error Return destinations require the page's own argument type.
  const invalidReturn: BackTo = { view: "projects", arg: { tab: "history" } };
  void invalidReturn;
  const projects: CenterView<"projects"> = {
    mount(_root, arg) {
      const project: string | null | undefined = arg?.project;
      void project;
      // @ts-expect-error Project arguments cannot contain an instructions tab.
      void arg?.tab;
    },
  };
  void projects;
}

test("typed locations preserve the native window targets", () => {
  assert.equal(mainTarget({ view: "home", arg: null }), "home");
  assert.deepEqual(mainTarget({ view: "projects", arg: null }), { projects: { project: null, run: null } });
  assert.deepEqual(mainTarget({ view: "projects", arg: { project: "acme/widget", run: { id: 42, host: "forge" } } }), { projects: { project: "acme/widget", run: { id: 42, host: "forge" } } });
  assert.equal(mainTarget({ view: "pane", arg: "settings" }), "settings");
  assert.equal(mainTarget({ view: "pane", arg: "agents" }), "home");
});
