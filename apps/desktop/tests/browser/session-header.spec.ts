import { expect, test } from "@playwright/test";
import type { Page } from "@playwright/test";
import type { AgentUsage } from "../../src/agents/agents-state.ts";
import { bundleFixture, openDesktop, sessionSettled } from "./harness.ts";

const agent = { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false };
const usages: Record<"short" | "long", AgentUsage> = {
  short: { context: null, tokens: 0, cost_usd: 0, credits: [{ provider: "openrouter", scope: "account", unit: "usd", remaining: null, total: null, used: null, resets: null }] },
  long: { context: { used: 250_000, size: 1_000_000 }, tokens: 54_200_000, cost_usd: 34.9 },
};
/// What the session-notes plugin places in the header: a text input and a Save button.
const notes = { kind: "row", gap: 6, children: [
  { kind: "input", label: "Session note", value: "", placeholder: "Add a note", onChange: 3, onSubmit: 4 },
  { kind: "button", label: "Save note", children: [{ kind: "text", text: "Save", tone: "normal" }], onPress: 5 },
] };
const placement = { plugin: "session-notes", slot: "session.header", instance: "build-box:notice-one", position: { kind: "inline" }, enter: "none", leave: "none", node: notes };

/// Where the header's parts are, and whether anything in or above it was scrolled.
async function layout(page: Page) {
  return page.evaluate(() => {
    const head = document.querySelector<HTMLElement>(".ag-head");
    if (!head) throw new Error("no session header");
    const scrolled: string[] = [];
    for (let el: Element | null = head; el; el = el.parentElement) if (el.scrollLeft !== 0 || el.scrollTop !== 0) scrolled.push(`${el.tagName}.${el.className}`);
    for (const el of head.querySelectorAll(".pluginhere, .pluginrow")) if (el.scrollLeft !== 0 || el.scrollTop !== 0) scrolled.push(`${el.tagName}.${el.className}`);

    const rect = (el: Element | null) => { const r = el?.getBoundingClientRect(); return r ? { left: r.left, right: r.right, width: r.width } : null; };
    const shown = (el: Element | null) => el !== null && getComputedStyle(el).display !== "none" && el.getBoundingClientRect().width > 0;
    const parts = [".dcoderav", ".tt", ".pluginhere", "#aguse", "#agstate"].flatMap(selector => {
      const el = head.querySelector(selector), r = el?.getBoundingClientRect();
      return el && r && shown(el) ? [{ selector, left: r.left, right: r.right, width: r.width }] : [];
    });
    const plugin = head.querySelector(".pluginhere");
    const tools = document.querySelector(".cc-top");
    return {
      scrolled, parts,
      head: rect(head), tools: rect(tools),
      headOverflow: head.scrollWidth - head.clientWidth,
      title: rect(head.querySelector(".tt b")),
      plugin: plugin && shown(plugin) ? { box: rect(plugin), clipped: plugin.scrollWidth - plugin.clientWidth, input: rect(plugin.querySelector("input")), button: rect(plugin.querySelector("button")) } : null,
      aguse: { clipped: (head.querySelector("#aguse")?.scrollWidth ?? 0) - (head.querySelector("#aguse")?.clientWidth ?? 0) },
    };
  });
}

for (const width of [1557, 1100, 900, 760]) for (const usage of ["short", "long"] as const) {
  test(`the session header fits at ${width}px with ${usage} usage and a focused plugin input`, async ({ page }, info) => {
    await page.setViewportSize({ width, height: 900 });
    await openDesktop(page, true);
    await page.evaluate(session => window.__navigation.open({ agent: session }), agent);
    await expect(page.locator("#agfield")).toBeVisible();
    await sessionSettled(page);
    await page.evaluate(value => window.__navigation.sessionUsage("notice-one", value), usages[usage]);
    await expect(page.locator("#aguse")).not.toBeEmpty();
    await page.evaluate(list => window.__navigation.emit("plugins-changed", list), [placement]);
    await expect(page.locator(".ag-head .pluginhere")).toBeAttached();

    // Typing into the input, and moving to the button, must not move the header.
    const field = page.getByRole("textbox", { name: "Session note" });
    if (await field.isVisible()) {
      await field.focus();
      await page.keyboard.type("a long note that goes on well past the end of the box, to move the caret");
      await page.keyboard.press("Tab");
    }
    // HEADER_SHOTS=<dir> keeps a picture of the row for each case.
    const shots = process.env.HEADER_SHOTS;
    if (shots) await page.screenshot({ path: `${shots}/${info.project.name}-${width}-${usage}.png`, clip: { x: 0, y: 0, width, height: 60 } });
    const found = await layout(page);
        expect(found.scrolled).toEqual([]);

    // The title keeps room to read, and the usage and state are whole.
    expect(found.title?.width ?? 0).toBeGreaterThan(80);
    if (width >= 1100) expect(found.aguse.clipped).toBeLessThanOrEqual(0);
    expect(found.headOverflow).toBeLessThanOrEqual(0);

    // Nothing overlaps its neighbour, and the row keeps clear of the tools laid over its end.
    for (const [i, a] of found.parts.entries()) for (const b of found.parts.slice(i + 1)) expect(a.right <= b.left + 0.5 || b.right <= a.left + 0.5, `${a.selector} overlaps ${b.selector}`).toBe(true);
    const state = found.parts.find(part => part.selector === "#agstate");
    expect(state && found.tools ? state.right <= found.tools.left + 0.5 : true).toBe(true);

    // The plugin's controls either all fit in their box, or the plugin is not shown.
    if (found.plugin) {
      expect(found.plugin.clipped).toBeLessThanOrEqual(0);
      expect(found.plugin.input?.width ?? 0).toBeGreaterThan(40);
      expect(found.plugin.button?.width ?? 0).toBeGreaterThan(20);
    }
    if (width >= 1100) expect(found.plugin).not.toBeNull();
  });
}

test("session spend updates before the running turn finishes", async ({ page }) => {
  await openDesktop(page, true);
  await page.evaluate(session => window.__navigation.open({ agent: session }), agent);
  await sessionSettled(page);
  const shown = page.locator("#aguse");
  const spend = (cost: number) => page.evaluate(cost_usd => {
    window.__navigation.sessionUsage("notice-one", { context: null, tokens: 0, cost_usd });
  }, cost);

  await spend(0.25);
  await expect(shown).toHaveText("$0.25");
  await spend(1.50);
  await expect(shown).toHaveText("$1.50");
  await expect(page.locator("#agstate")).toContainText(/working/i);
});

/// Every element from `target` up to the window, and the ones inside it, that was scrolled.
const scrolledFrom = (page: Page, selector: string) => page.evaluate(selector => {
  const found: string[] = [];
  for (let el: Element | null = document.querySelector(selector); el; el = el.parentElement) if (el.scrollLeft !== 0 || el.scrollTop !== 0) found.push(`${el.tagName}.${el.className}`);
  return found;
}, selector);

test("a plugin input in the composer slot does not scroll the page when it takes focus", async ({ page }) => {
  await page.setViewportSize({ width: 760, height: 900 });
  await openDesktop(page, true, {});
  await page.evaluate(session => window.__navigation.open({ agent: session }), agent);
  await expect(page.locator("#agfield")).toBeVisible();
  const edge = { kind: "edge", align: "end", offset: 84, sink: 3 };
  await page.evaluate(list => window.__navigation.emit("plugins-changed", list), [{ ...placement, slot: "composer", instance: "composer", position: edge }]);
  const field = page.getByRole("textbox", { name: "Session note" });
  await field.focus();
  await page.keyboard.type("a long note that goes on well past the end of the box, to move the caret");
  expect(await scrolledFrom(page, ".pluginslot")).toEqual([]);
  expect(await scrolledFrom(page, "#agfield")).toEqual([]);
});

test("a long note in a session row leaves the title and state in place", async ({ page }) => {
  const rows = [0, 1].map(index => ({ id: `agent-${index}`, machine_id: "build-box", machine_name: "Build box", agent: "codex", title: `Agent ${index}`, cwd: "/srv/project", status: "working", doing: "Working", started_at: index, last_activity: index, external: false, question: null }));
  await page.setViewportSize({ width: 760, height: 900 });
  await openDesktop(page, false, { agent_sessions: rows, session_moves: [] });
  await page.goto("https://app.test/index.html");
  const script = await bundleFixture("tests/browser/agents-bar-fixture.ts");
  await page.addScriptTag({ content: script });
  await page.evaluate(() => Reflect.get(window, "openTestAgents")());
  const note = { kind: "text", text: "a note that is much too long to fit in a narrow row of the session list", tone: "accent" };
  await page.evaluate(list => window.__navigation.emit("plugins-changed", list), [{ ...placement, slot: "session.row", instance: "build-box:agent-0", node: note }]);
  await expect(page.locator(".dchat .pluginhere")).toBeVisible();

  const found = await page.evaluate(() => {
    const row = document.querySelectorAll<HTMLElement>(".dchat")[0];
    const rect = (selector: string) => { const r = row.querySelector(selector)?.getBoundingClientRect(); return r ? { left: r.left, right: r.right } : null; };
    return { overflow: row.scrollWidth - row.clientWidth, scroll: row.scrollLeft, title: rect(".t"), plugin: rect(".pluginhere"), row: row.getBoundingClientRect().right };
  });
  expect(found.overflow).toBeLessThanOrEqual(0);
  expect(found.scroll).toBe(0);
  expect(found.title && found.plugin ? found.title.right <= found.plugin.left + 0.5 : false).toBe(true);
  expect(found.plugin ? found.plugin.right <= found.row + 0.5 : false).toBe(true);
});
