import { expect, test } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

const hosts = [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex", "claude"], workspace: "/srv/project", home: "/path/to/work", capabilities: [] }];
const models = { pick: "listed", current: "alpha", models: [
  { id: "alpha", name: "Alpha", description: null }, { id: "beta", name: "Beta", description: null },
] };

async function selectBeta(page: import("@playwright/test").Page) {
  const picker = page.getByRole("dialog", { name: "Harness, model and thinking" });
  await expect(picker).toBeVisible();
  const input = picker.locator(".agc-search");
  await input.fill("beta");
  await expect(picker.locator('[data-agmo="beta"]')).toBeVisible();
  await input.press("ArrowDown");
  await expect(picker.locator('[data-agmo="beta"]')).toHaveClass(/hi/);
  await input.press("Enter");
  await expect(picker).toHaveCount(0);
}

test("project composer picker filters and selects a model", async ({ page }) => {
  const errors: string[] = [];
  page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
  await openDesktop(page, false, { agent_hosts: hosts, agent_models: models });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  const form = page.locator(".cc-pjc:visible");
  const pill = form.getByTitle("Harness, model and thinking");
  await expect(pill).toBeEnabled();
  await pill.click();
  await selectBeta(page);
  await expect(pill).toContainText("Beta");
  expect(errors.filter(message => message.includes("flushSync") || message.includes("unmount"))).toEqual([]);
});

test("session model picker filters and closes after selection", async ({ page }) => {
  await openDesktop(page, true, { agent_hosts: hosts, agent_models: models });
  await page.evaluate(() => window.__navigation.open({ agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session",
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  } }));
  const pill = page.locator("#agmodelb");
  await pill.evaluate(el => { el.dataset.identity = "kept"; });
  await pill.click();
  await expect(pill).toHaveAttribute("aria-expanded", "true");
  await selectBeta(page);
  await expect(pill).toHaveAttribute("data-identity", "kept");
  await expect(pill).toHaveAttribute("aria-expanded", "false");
});

test("new-agent picker changes harness, effort and model", async ({ page }, info) => {
  await openDesktop(page, false, { agent_hosts: hosts, agent_models: models });
  await page.keyboard.press(info.project.name === "Windows" ? "Control+n" : "Meta+n");
  const pill = page.locator("#agpillb");
  await expect(pill).toBeEnabled();
  await pill.click();
  const picker = page.getByRole("dialog", { name: "Harness, model and thinking" });
  await expect(picker).toBeVisible();
  await picker.locator('[data-agh="codex"]').click();
  await expect(picker.locator('[data-agh="codex"]')).toHaveAttribute("aria-pressed", "true");
  await picker.locator('[data-age="high"]').click();
  await expect(picker.locator('[data-age="high"]')).toHaveClass(/on/);
  await selectBeta(page);
  await expect(pill).toContainText("Beta");
});

test("model results preserve a typed query, caret and input identity", async ({ page }) => {
  await openDesktop(page, false, { agent_hosts: hosts, agent_models: models });
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command === "agent_models") await new Promise<void>(resolve => window.addEventListener("test-models-ready", () => resolve(), { once: true }));
      return invoke<T>(command, args);
    };
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  const pill = page.locator(".cc-pjc:visible").getByTitle("Harness, model and thinking");
  await expect(pill).toBeEnabled();
  await pill.click();
  const picker = page.getByRole("dialog", { name: "Harness, model and thinking" });
  const input = picker.locator(".agc-search");
  await input.fill("beta");
  await input.evaluate(element => { if (element instanceof HTMLInputElement) { element.dataset.identity = "same"; element.setSelectionRange(2, 2); } });
  await page.evaluate(() => window.dispatchEvent(new Event("test-models-ready")));
  await expect(picker.locator('[data-agmo="beta"]')).toBeVisible();
  await expect(input).toHaveValue("beta");
  await expect(input).toHaveAttribute("data-identity", "same");
  await expect(input).toBeFocused();
  await expect(input).toHaveJSProperty("selectionStart", 2);
});

test("delayed model results clear the previous keyboard highlight", async ({ page }) => {
  await page.addInitScript(cached => {
    const entry = { list: cached, at: Date.now() };
    localStorage.setItem("beep.agentModels.v1", JSON.stringify({ "build-box/claude": entry, "build-box/codex": entry }));
  }, { pick: "listed", current: "alpha", models: [{ id: "alpha", name: "Alpha", description: null }] });
  const updated = { pick: "listed", current: "beta", models: [{ id: "beta", name: "Beta", description: null }] };
  await openDesktop(page, false, { agent_hosts: hosts, agent_models: updated });
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command === "agent_models") await new Promise<void>(resolve => window.addEventListener("test-models-ready", () => resolve(), { once: true }));
      return invoke<T>(command, args);
    };
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  const pill = page.locator(".cc-pjc:visible").getByTitle("Harness, model and thinking");
  await expect(pill).toBeEnabled();
  await pill.click();
  const picker = page.getByRole("dialog", { name: "Harness, model and thinking" });
  const input = picker.locator(".agc-search");
  await expect(picker.locator('[data-agmo="alpha"]')).toBeVisible();
  await input.press("ArrowDown");
  await input.press("ArrowDown");
  await expect(picker.locator('[data-agmo="alpha"]')).toHaveClass(/hi/);
  await page.evaluate(() => window.dispatchEvent(new Event("test-models-ready")));
  await expect(picker.locator('[data-agmo="beta"]')).toBeVisible();
  await expect(picker.locator(".hi")).toHaveCount(0);
  await input.press("Enter");
  await expect(picker).toHaveCount(0);
  await pill.click();
  await expect(picker.locator('[data-agmo=""]')).toHaveClass(/on/);
  await expect(picker.locator('[data-agmo="beta"]')).not.toHaveClass(/on/);
});

test("new-agent picker can be reopened after its view is removed", async ({ page }, info) => {
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  await openDesktop(page, false, { agent_hosts: hosts, agent_models: models });
  const shortcut = info.project.name === "Windows" ? "Control+n" : "Meta+n";
  await page.keyboard.press(shortcut);
  await page.locator("#agpillb").click();
  await expect(page.getByRole("dialog", { name: "Harness, model and thinking" })).toBeVisible();
  await page.evaluate(() => window.__navigation.open({ agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session",
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  } }));
  await expect(page.getByRole("dialog", { name: "Harness, model and thinking" })).toHaveCount(0);
  await page.evaluate(() => window.__navigation.open("home"));
  await page.keyboard.press(shortcut);
  await page.locator("#agpillb").click();
  await expect(page.getByRole("dialog", { name: "Harness, model and thinking" })).toBeVisible();
  expect(errors).toEqual([]);
});
