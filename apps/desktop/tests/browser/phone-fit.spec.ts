import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

// Long names and running sessions with long activity lines: what can push a
// phone page wider than the screen.
const long = "a-very-long-repository-name-that-keeps-going-without-any-break-at-all";
const machines = [{ id: "box-0", name: "build-box" }, { id: "box-1", name: "gpu-box" }];
const projects = machines.map((m, i) => ({ name: `example/${long}-${i}`, url: `https://example.test/example/${i}`, machine_id: m.id, machine_name: m.name, folder: `/srv/work/${long}-${i}`, cloned: true, imported_at: 1 }));
const sessions = ["claude", "codex", "claude", "codex"].map((agent, i) => ({
  kind: "agent", machine_id: "box-0", machine_name: "build-box", id: `session-${i}`, agent, title: `A session title that keeps going ${long}`, cwd: `/srv/work/${long}-0`,
  status: i < 2 ? "working" : "idle", doing: i < 2 ? `Running a long command with many arguments ${long}` : null, started_at: 1, last_activity: 1, external: false, question: null,
}));
const responses = {
  center_home: { machines: machines.map(m => ({ ...m, online: true, can_host: true, seen_at: 0, version: "1", env: null, joined: true, apps: [], gpus: [], capabilities: [], agents: 0, stats: null })), projects, running: sessions, needs_you: [], coming_up: null, updates: null },
  agent_sessions: sessions, project_sessions: sessions, list_sessions: sessions, mobile_paired: true,
};

async function openPhone(page: Page) {
  await page.setViewportSize({ width: 402, height: 874 });
  await openDesktop(page, false, responses);
  await expect(page.locator("body")).toHaveClass(/mobile/);
}

const tab = (page: Page, name: string) => page.getByRole("navigation", { name: "Sections" }).getByRole("button", { name, exact: true });

/// The window's sideways overflow, and boxes that scroll sideways without being meant to.
/// The machines strip, the session tabs and the header tools scroll on purpose.
const sideways = (page: Page) => page.evaluate(() => {
  const out: string[] = [];
  // The body clips but can still be scrolled sideways, and iOS does.
  for (const box of [document.documentElement, document.body]) if (box.scrollWidth > box.clientWidth) out.push(`${box.tagName.toLowerCase()} ${box.scrollWidth} > ${box.clientWidth}`);
  const meant = ".cc-mstrip, .agmnav, .cc-top .tools, .pj-strip, .cc-mt, pre, .dcoderen";
  for (const el of document.querySelectorAll<HTMLElement>(".cwin *")) {
    if (!el.getClientRects().length || el.closest(meant)) continue;
    if (/auto|scroll/.test(getComputedStyle(el).overflowX) && el.scrollWidth > el.clientWidth + 1) out.push(`${el.tagName.toLowerCase()}.${String(el.className)} ${el.scrollWidth} > ${el.clientWidth}`);
  }
  return out;
});

test("no phone tab scrolls sideways", async ({ page }) => {
  await openPhone(page);
  for (const name of ["Home", "Projects", "Agents", "Terminals", "Machines"]) {
    await tab(page, name).click();
    // Machines is a part of Home.
    await expect(tab(page, name === "Machines" ? "Home" : name)).toHaveAttribute("aria-current", "true");
    expect(await sideways(page), name).toEqual([]);
  }
});

test("phone agent rows show their harness and truncate, with New in the header", async ({ page }) => {
  await openPhone(page);
  await tab(page, "Agents").click();
  const rows = page.locator("[data-agi]");
  await expect(rows).toHaveCount(sessions.length);
  await expect(rows.locator(".dcoderav")).toHaveCount(sessions.length);
  await expect(rows.first().locator(".dcoderav")).toHaveClass(/claude/);
  await expect(rows.nth(1).locator(".dcoderav")).toHaveClass(/codex/);
  await expect(page.locator("#agNew")).toBeHidden();
  await expect(page.locator(".cc-top .ag-new-top")).toBeVisible();

  const width = await page.evaluate(() => document.documentElement.clientWidth);
  for (const box of await rows.locator(".t > span").evaluateAll(spans => spans.map(span => span.getBoundingClientRect().right))) expect(box).toBeLessThanOrEqual(width);
  expect(await sideways(page)).toEqual([]);

  await page.locator(".cc-top .ag-new-top").click();
  await expect(page.locator("#title")).toHaveText("Start a coding agent");
});

test("a swipe back never lets the phone window scroll sideways", async ({ page }) => {
  await openPhone(page);
  await tab(page, "Agents").click();
  await page.locator("[data-agi]").first().click();
  await expect(page.locator(".mb-back")).toBeVisible();
  const widths = await page.evaluate(() => {
    const touch = (x: number) => new Touch({ identifier: 1, target: document.body, clientX: x, clientY: 400 });
    const fire = (type: string, x: number) => document.body.dispatchEvent(new TouchEvent(type, { bubbles: true, touches: type === "touchend" ? [] : [touch(x)], changedTouches: [touch(x)] }));
    fire("touchstart", 8);
    for (const x of [40, 90, 140]) fire("touchmove", x);
    const mid = document.body.scrollWidth;
    fire("touchend", 140);
    return { mid, client: document.body.clientWidth };
  });
  expect(widths.mid).toBeLessThanOrEqual(widths.client);
  await expect(page.locator(".mb-back")).toBeHidden();
  expect(await sideways(page)).toEqual([]);
});

test("the phone composer is one surface", async ({ page }) => {
  await openPhone(page);
  await tab(page, "Agents").click();
  await page.locator("[data-agi]").first().click();
  const field = page.locator("#agfield");
  await expect(field).toBeVisible();
  const look = await field.evaluate(box => {
    const outer = getComputedStyle(box), text = getComputedStyle(box.querySelector("textarea") ?? box);
    return { radius: outer.borderTopLeftRadius, top: outer.borderTopWidth, left: outer.borderLeftWidth, text: text.borderTopWidth, textBg: text.backgroundColor };
  });
  expect(look).toEqual({ radius: "12px", top: "1px", left: "1px", text: "0px", textBg: "rgba(0, 0, 0, 0)" });
  await page.locator("#agmsg").focus();
  await expect(page.getByRole("button", { name: "Hide the keyboard" })).toBeVisible();
  expect(await sideways(page)).toEqual([]);
});

for (const [layout, width] of [["phone", 402], ["desktop", 1280]] as const) {
  test(`project groups fold and stay folded on the ${layout}`, async ({ page }) => {
    await page.setViewportSize({ width, height: 874 });
    await openDesktop(page, false, responses);
    await page.evaluate(() => window.__navigation.open({ projects: { project: null } }));
    const head = page.locator(".pj-org .pj-fold", { hasText: "example/" });
    const cards = page.locator(".pj-card");
    await expect(cards).toHaveCount(projects.length);

    await head.click();
    await expect(head).toHaveAttribute("aria-expanded", "false");
    await expect(cards).toHaveCount(0);

    // A search shows the matches of a folded group.
    await page.getByRole("searchbox", { name: "Search projects" }).fill("-1");
    await expect(cards).toHaveCount(1);
    await page.getByRole("searchbox", { name: "Search projects" }).fill("");
    await expect(cards).toHaveCount(0);

    await page.reload();
    await page.waitForFunction(() => window.__navigation.ready());
    await page.evaluate(() => window.__navigation.open({ projects: { project: null } }));
    await expect(head).toHaveAttribute("aria-expanded", "false");
    await expect(cards).toHaveCount(0);
    await head.click();
    await expect(cards).toHaveCount(projects.length);
  });
}

test("the phone Agents list shows a session that starts while it is open", async ({ page }) => {
  await page.setViewportSize({ width: 402, height: 874 });
  await openDesktop(page, false, { ...responses, agent_sessions: sessions.slice(2) });
  await tab(page, "Agents").click();
  const rows = page.locator("[data-agi]");
  await expect(rows).toHaveCount(2);

  await page.evaluate(list => { window.__navigation.respond("agent_sessions", list); window.__navigation.emit("sessions-changed", null); }, sessions);
  await expect(rows).toHaveCount(sessions.length);
});
