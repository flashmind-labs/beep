import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const hosts = [
  { machine_id: "build-box", machine_name: "build-box", agents: ["codex", "claude"], workspace: "/srv/project", home: "/path/to/work", capabilities: [] },
];
const models = { pick: "listed", current: "alpha", models: [{ id: "alpha", name: "Alpha", description: null }] };
const row = {
  machine_id: "build-box", machine_name: "build-box", cwd: "/srv/project", external: false, doing: null, question: null,
  started_at: 1, last_activity: 1, id: "early", agent: "codex", title: "Plan the work", status: "idle",
};

test("a new session opened without visiting Projects redraws its bar", async ({ page }, info) => {
  await openDesktop(page, false, { agent_hosts: hosts, agent_models: models, agent_sessions: [row] });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), row);
  await page.keyboard.press(info.project.name === "macOS" ? "Meta+n" : "Control+n");
  await expect(page.locator("#agdraft textarea")).toBeFocused();
  const pill = page.locator("#agdraft").getByTitle("Harness, model and thinking");
  await expect(pill).toBeEnabled();

  await pill.click();
  const picker = page.getByRole("dialog", { name: "Harness, model and thinking" });
  await expect(picker).toBeVisible();
  await picker.locator(".agc-search").fill("codex alpha");
  await picker.locator('[data-agmo="alpha"]').click();
  await expect(picker).toHaveCount(0);
  await expect(pill).toContainText("Alpha");
});
