import { readFileSync } from "node:fs";
import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

// The script the app puts at the start of every HTML page it serves.
const paneKeys = /const PANE_KEYS: &\[u8\] = br#"(.*)"#;/.exec(readFileSync(new URL("../../src-tauri/src/agents/artifacts.rs", import.meta.url), "utf8"))?.[1] ?? "";

test("a focused HTML preview hands ⇧esc, ⌘1 and ⌘W to the pane", async ({ page }, info) => {
  expect(paneKeys).toContain("<script>");
  const command = info.project.name === "Windows" ? "Control" : "Meta";
  await page.addInitScript(() => localStorage.setItem("beep.projectView.build-box\nnotice-one", JSON.stringify({
    on: { right: "text:page.html", bottom: "text:page.html" }, tabs: [{ root: "/srv/project", rel: "page.html" }],
  })));
  await openDesktop(page, true, { agent_artifact: { kind: "html", size: 30, src: `data:text/html,${encodeURIComponent(`${paneKeys}<p>Preview</p>`)}` } });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex",
    title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  const tab = page.getByRole("tab", { name: /^page.html Close/ });
  await expect(tab).toHaveAttribute("aria-selected", "true");
  await page.getByRole("button", { name: "Preview", exact: true }).click();
  const preview = page.frameLocator(".agartframe").getByText("Preview");

  await preview.click();
  await page.keyboard.press("Shift+Escape");
  await expect(page.locator("#agshell")).toHaveClass(/zoomed/);
  await preview.click();
  await page.keyboard.press(`${command}+1`);
  await expect(page.locator("#agshell")).not.toHaveClass(/zoomed/);
  await expect(page.locator("#agmsg")).toBeFocused();

  await preview.click();
  await page.keyboard.press(`${command}+w`);
  await expect(tab).toHaveCount(0);
});
