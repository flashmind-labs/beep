import { expect, test } from "@playwright/test";
import { openDesktop, openStartForm, projectName } from "./harness.ts";

const hosts = [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex", "claude"], workspace: "/srv/project", home: "/path/to/work", capabilities: [] }];
const models = { pick: "listed", current: "alpha", models: [
  { id: "alpha", name: "Alpha", description: null }, { id: "beta", name: "Beta", description: null },
] };

/// Picks Beta under Codex, then the thinking level `effort`, or keeps the
/// one picked with Enter.
async function selectBeta(page: import("@playwright/test").Page, effort: string | null = null) {
  const picker = page.getByRole("dialog", { name: "Machine, harness, model and thinking" });
  await expect(picker).toBeVisible();
  const input = picker.locator(".agc-search");
  // Both harnesses list Beta: naming one keeps the search on it.
  await input.fill("codex beta");
  await expect(picker.locator('[data-agmo="beta"]')).toBeVisible();
  await input.press("ArrowDown");
  await expect(picker.locator('[data-agmo="beta"]')).toHaveClass(/hi/);
  await input.press("Enter");
  // The Thinking step comes next, its level focused.
  await expect(picker.locator('[data-agstep="thinking"]')).toHaveAttribute("aria-current", "step");
  if (effort === null) await page.keyboard.press("Enter");
  else await picker.locator(`[data-age="${effort}"]`).click();
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

test("the picker opens beside its pill, inside the window", async ({ page }) => {
  await openDesktop(page, false, { agent_hosts: hosts, agent_models: models });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  const pill = page.locator(".cc-pjc:visible").getByTitle("Harness, model and thinking");
  await pill.click();
  const picker = page.getByRole("dialog", { name: "Machine, harness, model and thinking" });
  await expect(picker).toBeVisible();
  const box = await picker.boundingBox(), anchor = await pill.boundingBox(), view = page.viewportSize();
  if (!box || !anchor || !view) throw new Error("the picker and its pill are laid out");
  expect(box.x).toBeGreaterThanOrEqual(0);
  expect(box.x + box.width).toBeLessThanOrEqual(view.width);
  expect(box.y + box.height).toBeLessThanOrEqual(view.height);
  expect(Math.min(Math.abs(box.y - (anchor.y + anchor.height)), Math.abs(box.y + box.height - anchor.y))).toBeLessThan(16);
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

test("new-agent picker changes harness, effort and model", async ({ page }) => {
  await openDesktop(page, false, { agent_hosts: hosts, agent_models: models });
  await openStartForm(page);
  const pill = page.locator("#agpillb");
  const choices = page.locator(".cc-pjc.centered > .choices");
  await expect(choices).toHaveCSS("display", "flex");
  await expect(pill).toBeEnabled();
  // The key hints under the form give way while the picker is open.
  const keys = page.locator("#agcomposer ~ .prompt-stash-bar");
  await expect(keys).toBeVisible();
  await pill.click();
  const picker = page.getByRole("dialog", { name: "Machine, harness, model and thinking" });
  await expect(picker).toBeVisible();
  await expect(keys).toBeHidden();
  // It opens on the model, the machine and harness filled in.
  await expect(picker.locator('[data-agstep="model"]')).toHaveAttribute("aria-current", "step");
  await expect(picker.locator('[data-agstep="machine"]')).toContainText("build-box");
  await picker.locator('[data-agstep="harness"]').click();
  await picker.locator('[data-agh="codex"]').click();
  await expect(picker.locator('[data-agstep="harness"]')).toContainText("Codex");
  await selectBeta(page, "high");
  await expect(pill).toContainText("Beta");
  await expect(pill).toContainText("High");
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
  const picker = page.getByRole("dialog", { name: "Machine, harness, model and thinking" });
  const input = picker.locator(".agc-search");
  await input.fill("codex beta");
  await input.evaluate(element => { if (element instanceof HTMLInputElement) { element.dataset.identity = "same"; element.setSelectionRange(2, 2); } });
  await page.evaluate(() => window.dispatchEvent(new Event("test-models-ready")));
  await expect(picker.locator('[data-agmo="beta"]')).toBeVisible();
  await expect(input).toHaveValue("codex beta");
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
  const picker = page.getByRole("dialog", { name: "Machine, harness, model and thinking" });
  const input = picker.locator(".agc-search");
  await expect(picker.locator('[data-agmo="alpha"]')).toBeVisible();
  await input.press("ArrowDown");
  await input.press("ArrowDown");
  await expect(picker.locator('[data-agmo="alpha"]')).toHaveClass(/hi/);
  await page.evaluate(() => window.dispatchEvent(new Event("test-models-ready")));
  await expect(picker.locator('[data-agmo="beta"]')).toBeVisible();
  await expect(picker.locator(".hi")).toHaveCount(0);
  await input.press("Enter");
  await page.keyboard.press("Enter");
  await expect(picker).toHaveCount(0);
  await pill.click();
  await expect(picker.locator('[data-agmo=""]')).toHaveClass(/on/);
  await expect(picker.locator('[data-agmo="beta"]')).not.toHaveClass(/on/);
});

test("new-agent picker can be reopened after its view is removed", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  await openDesktop(page, false, { agent_hosts: hosts, agent_models: models });
  await openStartForm(page);
  await page.locator("#agpillb").click();
  await expect(page.getByRole("dialog", { name: "Machine, harness, model and thinking" })).toBeVisible();
  await page.evaluate(() => window.__navigation.open({ agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session",
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  } }));
  await expect(page.getByRole("dialog", { name: "Machine, harness, model and thinking" })).toHaveCount(0);
  await openStartForm(page);
  await page.locator("#agpillb").click();
  await expect(page.getByRole("dialog", { name: "Machine, harness, model and thinking" })).toBeVisible();
  expect(errors).toEqual([]);
});

test("new agent draft and caret survive machine discovery", async ({ page }) => {
  await openDesktop(page, false, { agent_hosts: hosts, agent_models: models });
  await page.evaluate(() => window.__navigation.holdNext("agent_hosts"));
  await openStartForm(page);
  const input = page.locator("#agprompt");
  await expect(input).toBeVisible();
  await input.fill("Keep this draft");
  await input.evaluate(el => { if (el instanceof HTMLTextAreaElement) { el.setSelectionRange(2, 5); Reflect.set(window, "startComposerInput", el); } });
  await page.evaluate(() => window.__navigation.release("agent_hosts"));
  await expect(page.locator("#agpillb")).toBeEnabled();
  expect(await input.evaluate(el => el === Reflect.get(window, "startComposerInput") && document.activeElement === el && el instanceof HTMLTextAreaElement && el.value === "Keep this draft" && el.selectionStart === 2 && el.selectionEnd === 5)).toBe(true);
});

test("new-agent machine and folder menus keep their choices", async ({ page }) => {
  await openDesktop(page, false, { agent_hosts: hosts, agent_models: models, list_machine_folder: { folders: ["project", "projects"] } });
  await openStartForm(page);
  await expect(page.locator("#aghostb")).toContainText("build-box");
  await page.locator("#aghostb").click();
  await expect(page.locator('#agunder [data-agm="build-box"]')).toBeVisible();
  await page.locator('#agunder [data-agm="build-box"]').click();
  await page.locator("#agfoldb").click();
  await expect(page.locator('#agunder [data-agtype="1"]')).toBeVisible();
  await expect(page.locator('#agunder [data-agf=""]')).toContainText("Workspace");
  await page.locator('#agunder [data-agtype="1"]').click();
  const folder = page.locator("#agcwd");
  await folder.fill("/srv/pro");
  await expect(page.locator('#agcwdmenu [role="option"]')).toHaveCount(2);
  await folder.evaluate(el => { if (el instanceof HTMLInputElement) { el.setSelectionRange(2, 4); Reflect.set(window, "folderInput", el); } });
  await page.keyboard.press("ArrowDown");
  expect(await folder.evaluate(el => el === Reflect.get(window, "folderInput") && document.activeElement === el && el instanceof HTMLInputElement && el.selectionStart === 2 && el.selectionEnd === 4)).toBe(true);
  await page.keyboard.press("Enter");
  await expect(folder).toHaveValue("/srv/projects/");
});

test("model search keeps its query, focus and highlight while lists stream in (#564)", async ({ page }) => {
  const listed = (ids: string[]) => ({ pick: "listed", current: ids[0], models: ids.map(id => ({ id, name: id.replace("-", " "), description: null })) });
  await page.addInitScript(cached => {
    const entry = { list: cached, at: Date.now() };
    localStorage.setItem("beep.agentModels.v1", JSON.stringify({ "build-box/claude": entry, "build-box/codex": entry }));
  }, listed(["haiku-4", "opus-5.5"]));
  await openDesktop(page, false, { agent_hosts: hosts, agent_models: listed(["haiku-4", "opus-5.5", "opus-5.5-fast"]) });
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command === "agent_models") await new Promise<void>(resolve => window.addEventListener("test-models-ready", () => resolve(), { once: true }));
      return invoke<T>(command, args);
    };
  });
  // The project page is still reading its machines, git state and branches.
  await page.evaluate(() => { for (const command of ["project_machines", "project_git_state", "project_branches"]) window.__navigation.holdNext(command); });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  const pill = page.locator(".cc-pjc:visible").getByTitle("Harness, model and thinking");
  await expect(pill).toBeEnabled();
  await pill.click();
  const picker = page.getByRole("dialog", { name: "Machine, harness, model and thinking" });
  const input = picker.locator(".agc-search");
  await expect(input).toBeFocused();
  await input.evaluate(el => Reflect.set(window, "modelSearch", el));

  // Each part is typed with focus left where it was, then a list or the page updates.
  const hostsWith = (agents: string[]) => [{ machine_id: "build-box", machine_name: "build-box", agents, workspace: "/srv/project", home: "/path/to/work", capabilities: [] }];
  const updates = [
    () => page.evaluate(() => { window.__navigation.release("project_machines"); window.__navigation.notify("machines"); }),
    () => page.evaluate(() => { window.__navigation.release("project_git_state"); window.__navigation.notify("projects"); }),
    () => page.evaluate(next => { window.__navigation.respond("agent_hosts", next); window.__navigation.notify("machines"); }, hostsWith(["codex", "claude", "flsh"])),
    () => page.evaluate(() => { window.__navigation.release("project_branches"); window.__navigation.notify("projects"); }),
    () => page.evaluate(next => { window.__navigation.respond("agent_hosts", next); window.__navigation.notify("accounts"); }, hostsWith(["codex", "claude"])),
  ];
  for (const [index, part] of ["claude op", "us", " 5", ".", "5"].entries()) {
    await page.keyboard.type(part);
    await updates[index]();
  }
  await expect(picker.locator('[data-agmo="opus-5.5"]')).toBeVisible();
  await expect(input).toHaveValue("claude opus 5.5");
  await page.keyboard.press("ArrowDown");
  await expect(picker.locator('[data-agmo="opus-5.5"]')).toHaveClass(/hi/);

  // The machine's own list arrives with one more model.
  await page.evaluate(() => window.dispatchEvent(new Event("test-models-ready")));
  await expect(picker.locator('[data-agmo="opus-5.5-fast"]')).toBeVisible();
  await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => resolve())));

  await expect(input).toHaveValue("claude opus 5.5");
  await expect(input).toBeFocused();
  await expect(picker.locator('[data-agmo="opus-5.5"]')).toHaveClass(/hi/);
  expect(await input.evaluate(el => el === Reflect.get(window, "modelSearch") && el instanceof HTMLInputElement && el.selectionStart === 15)).toBe(true);
});

test("model rows are no Tab stops: Tab from the search leaves the list", async ({ page }) => {
  await openDesktop(page, false, { agent_hosts: hosts, agent_models: models });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await page.locator(".cc-pjc:visible").getByTitle("Harness, model and thinking").click();
  const picker = page.getByRole("dialog", { name: "Machine, harness, model and thinking" });
  const input = picker.locator(".agc-search");
  await expect(input).toBeFocused();
  await expect(picker.locator('[data-agmo="beta"]')).toBeVisible();
  await input.press("Tab");
  expect(await page.evaluate(() => document.activeElement?.hasAttribute("data-agmo"))).toBe(false);
  await input.focus();
  await input.press("ArrowDown");
  await expect(picker.locator(".agc-models .hi")).toHaveCount(1);
});

test("a resting cursor highlights no model row; a moved one picks its row on Enter", async ({ page }) => {
  await openDesktop(page, false, { agent_hosts: hosts, agent_models: models });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  const pill = page.locator(".cc-pjc:visible").getByTitle("Harness, model and thinking");
  await pill.click();
  const picker = page.getByRole("dialog", { name: "Machine, harness, model and thinking" });
  const beta = picker.locator('[data-agmo="beta"]');
  const box = await beta.boundingBox();
  if (!box) throw new Error("the Beta row has no box");
  await page.keyboard.press("Escape");
  await expect(picker).toHaveCount(0);

  // The picker opens from the keys, under a cursor already where Beta shows.
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await pill.focus();
  await page.keyboard.press("Enter");
  await expect(beta).toBeVisible();
  await expect(beta).toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
  await expect(picker.locator(".agc-models .hi")).toHaveCount(0);

  await page.mouse.move(box.x + box.width / 2 + 6, box.y + box.height / 2 + 2, { steps: 3 });
  await expect(beta).toHaveClass(/hi/);
  await page.keyboard.press("Enter");
  await page.keyboard.press("Enter");
  await expect(picker).toHaveCount(0);
  await expect(pill).toContainText("Beta");
});

test("⇧⌘M then `opus 5.5 h` and Enter picks the model and thinking across harnesses", async ({ page }, info) => {
  await openDesktop(page, false, { agent_hosts: hosts, agent_models: null });
  await page.evaluate(() => {
    const lists: Record<string, unknown> = {
      codex: { pick: "listed", current: null, models: [{ id: "gpt-6", name: "GPT-6", description: null }] },
      claude: { pick: "listed", current: null, models: [{ id: "opus", name: "Opus 5.5", description: null }, { id: "sonnet", name: "Sonnet 5.5", description: null }] },
    };
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      const agent = args?.agent;
      if (command === "agent_models" && typeof agent === "string") return lists[agent] as T;
      return invoke<T>(command, args);
    };
  });
  const mod = info.project.name === "Windows" ? "Control" : "Meta";
  await openStartForm(page);
  const pill = page.locator("#agpillb");
  await expect(pill).toBeEnabled();
  await page.locator("#agprompt").press(`${mod}+Shift+m`);
  const picker = page.getByRole("dialog", { name: "Machine, harness, model and thinking" });
  await expect(picker).toBeVisible();
  await picker.locator('[data-agstep="harness"]').click();
  await picker.locator('[data-agh="codex"]').click();
  // A search that names the thinking level skips the Thinking step.
  await page.keyboard.type("opus 5.5 h");
  await expect(picker.locator('[data-agmo="opus"]')).toBeVisible();
  await expect(picker.locator('[data-agmo="opus"] .cc-ag.claude')).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(picker).toHaveCount(0);
  await expect(pill).toContainText("High Opus 5.5");
  await expect(pill.locator(".cc-ag.claude")).toBeVisible();
});
