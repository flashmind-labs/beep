import { expect, test, type Locator } from "@playwright/test";
import { openDesktop, openStartForm, projectName } from "./harness.ts";

const LONG = Array.from({ length: 30 }, (_, i) => `line ${i + 1}`).join("\n");

const session = (id: string) => ({
  kind: "agent", machine_id: "build-box", machine_name: "build-box", id, agent: "codex", title: id, cwd: "/srv/project",
  status: "working", doing: null, question: null, started_at: 1, last_activity: 1, external: false,
});

const layer = (field: Locator) => field.locator("xpath=preceding-sibling::div[contains(@class, 'cx-layer')][1]");

async function paste(field: Locator, text: string) {
  await field.evaluate((element, value) => {
    const data = new DataTransfer();
    data.setData("text/plain", value);
    element.dispatchEvent(new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true }));
  }, text);
}

async function clickPill(field: Locator) {
  const box = await layer(field).locator(".cx-pill").boundingBox();
  if (!box) throw new Error("the pill is laid out");
  await field.click({ position: await field.evaluate((element, b) => {
    const r = element.getBoundingClientRect();
    return { x: b.x + b.width / 2 - r.left, y: b.y + b.height / 2 - r.top };
  }, box) });
}

test("a long paste is one pill: previewed, expanded or removed whole", async ({ page }) => {
  await openDesktop(page);
  await openStartForm(page);
  const field = page.locator("#agprompt");
  await field.fill("Fix this:");
  await field.press("End");
  await paste(field, LONG);

  await expect(layer(field).locator(".cx-pill")).toContainText("Pasted text · 30 lines");
  await expect(field).not.toHaveValue(/line 1\n/);

  await clickPill(field);
  const pop = page.locator(".cx-pop");
  await expect(pop).toContainText("30 lines");
  await expect(pop.locator("pre")).toContainText("line 30");
  await page.mouse.click(5, 5);
  await expect(pop).toHaveCount(0);

  await clickPill(field);
  await field.press("Escape");
  await expect(pop).toHaveCount(0);

  await clickPill(field);
  await pop.getByRole("button", { name: "Expand" }).click();
  await expect(pop).toHaveCount(0);
  await expect(field).toHaveValue(`Fix this: ${LONG}`);
  await expect(layer(field).locator(".cx-pill")).toHaveCount(0);

  await field.fill("Fix this:");
  await field.press("End");
  await paste(field, LONG);
  await expect(layer(field).locator(".cx-pill")).toHaveCount(1);
  await field.press("End");
  await field.press("Backspace");
  await expect(field).toHaveValue("Fix this: ");
  await expect(layer(field)).toBeHidden();
});

test("text after a blank line is outlined as code", async ({ page }) => {
  await openDesktop(page);
  await openStartForm(page);
  const field = page.locator("#agprompt");
  await field.fill("a\n\n\nb");
  await expect(layer(field).locator(".cx-code")).toHaveCount(1);
  await field.fill("plain");
  await expect(layer(field)).toBeHidden();
});

test("each project keeps its own unsent task", async ({ page }) => {
  const other = "example/gadget";
  const row = (name: string) => ({ display_name: null, name, url: `https://example.test/${name}`, machine_id: "build-box", machine_name: "build-box", folder: "/srv/project", cloned: true });
  await openDesktop(page, false, {
    agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: null, home: "/srv", capabilities: [] }],
    center_home: { machines: [], projects: [row(projectName), row(other)], running: [], needs_you: [] },
  });
  const input = page.locator(".cc-pjc:visible").getByRole("textbox", { name: "Task for a coding agent" });
  const open = (project: string) => page.evaluate(name => window.__navigation.open({ projects: { project: name } }), project);

  await open(projectName);
  await input.pressSequentially("Widget task");
  const widgetField = await input.elementHandle();
  await open(other);
  await expect(input).toHaveAttribute("placeholder", /gadget/);
  await expect(input).toHaveValue("");
  expect(await widgetField.evaluate(field => field.isConnected)).toBe(false);
  await expect(input).toBeFocused();
  await expect(input).toHaveAttribute("data-prompt-stash", "");
  await input.fill("Gadget task");
  await open(projectName);
  await expect(input).toHaveValue("Widget task");
  await expect(input).toBeFocused();
  await input.pressSequentially(" continued");
  await expect(input).toHaveValue("Widget task continued");
  await page.reload();
  await page.waitForFunction(() => window.__navigation.ready());
  await open(other);
  await expect(input).toHaveValue("Gadget task");
});

test("each session keeps its own draft, a pill's full text included", async ({ page }) => {
  await openDesktop(page);
  const field = page.locator("#agmsg");
  const open = (id: string) => page.evaluate(row => window.__navigation.open({ agent: row }), session(id));
  await open("notice-one");
  await field.fill("kept draft ");
  await field.press("End");
  await paste(field, LONG);
  await expect(layer(field).locator(".cx-pill")).toHaveCount(1);

  await open("notice-two");
  await expect(field).toHaveValue("");
  await field.fill("other draft");

  await open("notice-one");
  await expect(field).toHaveValue(`kept draft ${LONG}`);
  await open("notice-two");
  await expect(field).toHaveValue("other draft");
});

test("sending clears the draft", async ({ page }) => {
  await openDesktop(page);
  const field = page.locator("#agmsg");
  const open = (id: string) => page.evaluate(row => window.__navigation.open({ agent: row }), session(id));
  await open("notice-one");
  await field.fill("send this");
  await field.press("Enter");
  await expect(field).toHaveValue("");
  await open("notice-two");
  await open("notice-one");
  await expect(field).toHaveValue("");
});

test("the message field's words, buttons and pills share one row", async ({ page }) => {
  const pi = { ...session("pi-one"), agent: "pi" };
  const model = "openrouter/anthropic/claude-sonnet-4.5";
  await openDesktop(page, false, {
    agent_view: { session: pi, model, plan: [], files: [], entries: [], earlier: 0, last_message: "", error: null, queued: 0, queued_messages: [], turns: 0, usage: null },
    agent_modes: { current: "default", modes: [{ id: "default", name: "Thinking: high" }, { id: "plan", name: "Plan" }], plan: null },
    agent_models: { pick: "listed", current: model, models: [{ id: model, name: "openrouter/Anthropic: Claude Sonnet 4.5", description: null, provider: null, efforts: ["low", "high"], fast: false }] },
  });
  await page.evaluate(row => window.__navigation.open({ agent: row }), pi);
  await expect(page.locator("#agmodeb")).toBeVisible();
  const centers = () => page.evaluate(() => {
    const mid = (selector: string) => {
      const r = document.querySelector(selector)?.getBoundingClientRect();
      return r ? r.top + r.height / 2 : null;
    };
    const field = document.querySelector("textarea#agmsg");
    if (!field) return null;
    const r = field.getBoundingClientRect(), lineHeight = parseFloat(getComputedStyle(field).lineHeight);
    return {
      words: r.bottom - lineHeight / 2,
      controls: ['#agfield button[aria-label="Attach files"]', "#agmodeb", "#agmodelb", "#agstop"].map(mid),
    };
  });
  const sameRow = async () => {
    const seen = await centers();
    if (!seen) throw new Error("the message field is laid out");
    for (const control of seen.controls) expect(Math.abs((control ?? 0) - seen.words)).toBeLessThan(1);
  };

  await sameRow();
  const pill = await page.locator("#agmodelb").boundingBox(), caret = await page.locator("#agmodelb > svg").boundingBox();
  expect(pill?.width).toBeLessThanOrEqual(180);
  expect((caret?.x ?? 0) + (caret?.width ?? 0)).toBeLessThanOrEqual((pill?.x ?? 0) + (pill?.width ?? 0));

  await page.locator("#agmsg").fill("one\ntwo\nthree");
  await sameRow();
});
