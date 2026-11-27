import test, { after } from "node:test";
import assert from "node:assert/strict";
import { closeWindow } from "./tauri-window.ts";
import { openScreen, screenWanted, type ScreenRequest } from "../src/center/nav.ts";

after(closeWindow);

test("a screen request reaches a follower with its data", () => {
  const seen: ScreenRequest[] = [];
  screenWanted.follow(request => { seen.push(request); });
  openScreen({ screen: "settings", section: "skills" });
  openScreen({ screen: "settings", section: null });
  openScreen({ screen: "importproject", repo: "org/repo", machine: "m1", openAfter: false });

  assert.deepEqual(seen, [
    { screen: "settings", section: "skills" },
    { screen: "settings", section: null },
    { screen: "importproject", repo: "org/repo", machine: "m1", openAfter: false },
  ]);
});
