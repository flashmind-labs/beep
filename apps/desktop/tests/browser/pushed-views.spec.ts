import { expect, test } from "@playwright/test";
import type { HarnessView } from "../../src/settings/providers-state.ts";
import { openDesktop, projectName } from "./harness.ts";

const checkout = (id: string, name: string) => ({ id, name, online: true, folder: "/srv/project", checking: false, checked_at: 1 });
const reads = (page: import("@playwright/test").Page, command: string) => page.evaluate(name => window.__navigation.calls().filter(call => call.command === name).length, command);

test("a project's checkouts come with the machines push and are not read again", async ({ page }) => {
  await openDesktop(page, false, { project_machines: [checkout("build-box", "build-box")] });
  await page.evaluate(name => window.__navigation.open({ projects: { project: name, tile: "machines" } }), projectName);
  await expect(page.locator(".pm-machine")).toHaveCount(1);
  const before = await reads(page, "project_machines");

  await page.evaluate(({ name, rows }) => window.__navigation.notify("machines", [
    { command: "project_machines", args: { project: name }, value: rows },
    { command: "project_syncthing", args: { project: name }, value: [] },
  ]), { name: projectName, rows: [checkout("build-box", "build-box"), checkout("studio", "studio")] });
  await expect(page.locator(".pm-machine")).toHaveCount(2);
  await expect(page.getByRole("article", { name: "studio", exact: true })).toBeVisible();
  expect(await reads(page, "project_machines")).toBe(before);
});

test("a machine's harnesses come with the machines push", async ({ page }) => {
  const harness = (version: string): HarnessView => ({
    agent: "claude", installed: true, runnable: true, version, latest: null, program: "claude", home_var: "CLAUDE_CONFIG_DIR", home_default: "~/.claude",
    sign_in: { state: "signed_in", account: null, plan: null, last4: null }, install: null,
    settings: { enabled: true, program: "", home: "", args: [], env: [] },
  });
  const view = (version: string) => ({ machine_id: "build", machine_name: "build-box", checked_at: null, harnesses: [harness(version)] });
  await openDesktop(page, false, {
    settings: { name: "build-box", version: "0.1", autostart: false, agent_notices: false },
    list_machines: [{ id: "build", name: "build-box", online: true }],
    agent_harnesses: view("1.0.0"),
    harness_settings_all: [], harness_compact: null, harness_model_compacts: [], harness_default_model: null, harness_driver: "acp", agent_models: null,
  });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("navigation", { name: "Settings pages" }).getByRole("button", { name: "Agents", exact: true }).click();
  await settings.getByRole("tab", { name: "Configuration", exact: true }).click();
  await settings.locator('#set-agent-overview [data-sub="agent:claude"]').click();
  await expect(settings).toContainText("1.0.0");
  const before = await reads(page, "agent_harnesses");

  await page.evaluate(next => window.__navigation.notify("machines", [{ command: "agent_harnesses", args: { machineId: "build", refresh: false }, value: next }]), view("2.5.0"));
  await expect(settings).toContainText("2.5.0");
  expect(await reads(page, "agent_harnesses")).toBe(before);
});
