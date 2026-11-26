import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("subagent pane shows native child actions and keeps rows on redraw", async ({ page }) => {
  await openDesktop(page, false, {
    agent_models: { pick: "listed", current: "gpt-example", models: [{ id: "gpt-example", name: "Example Model 1.0", description: null, provider: null }] },
  });
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      const value = await invoke<T>(command, args);
      if (command !== "agent_view" || !value || typeof value !== "object") return value;
      return { ...value, entries: [{ kind: "tool", text: "Inspect files [completed]\n[steps] 1\n[agent] Explorer\n[session] subagent:codex:child\n[model] gpt-example\nDone" }] } as T;
    };
  });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await page.getByRole("tab", { name: /Subagents/ }).click();
  const row = page.locator(".agpview.subs .dsubrow");
  await expect(row).toContainText("Inspect files");
  await expect(row).toContainText("Example Model 1.0");
  await expect(row).not.toContainText("gpt-example");
  await expect(row).toHaveAttribute("data-native", "0");
  await expect(row.locator("[data-subclose]")).toHaveCount(1);
  await row.focus();
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  await page.getByRole("tab", { name: /Subagents/ }).click();
  await expect(row).toBeVisible();
});

test("transcript groups child agents and keeps their actions", async ({ page }) => {
  await openDesktop(page);
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      const value = await invoke<T>(command, args);
      if (command !== "agent_view" || !value || typeof value !== "object") return value;
      return { ...value, entries: [{ kind: "user", text: "Check" }, { kind: "tool", text: "Inspect first [completed]\n[steps] 0\n[child] first@build-box working key-1" }, { kind: "tool", text: "Inspect second [completed]\n[steps] 0\n[child] second@build-box idle key-2" }] } as T;
    };
  });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  const group = page.locator(".dsubgrp");
  await expect(group).toContainText("2 agents started");
  const first = group.locator('[data-kid="first@build-box"]');
  await expect(first).toContainText("Inspect first");
  await expect(first.locator("[data-kidstop]")).toHaveCount(1);
  await expect(group.locator('[data-kid="second@build-box"]')).toContainText("Inspect second");
});

test("a child started after the list was pushed is waited for through pushes that do not list it", async ({ page }) => {
  await openDesktop(page);
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      const value = await invoke<T>(command, args);
      if (command !== "agent_view" || !value || typeof value !== "object") return value;
      return { ...value, entries: [{ kind: "user", text: "Check" }, { kind: "tool", text: "Inspect first [completed]\n[steps] 0\n[child] first@build-box working key-1" }] } as T;
    };
  });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  const kid = page.locator(".dsubgrp").locator('[data-kid="first@build-box"]');
  await expect(kid).toContainText("Inspect first");
  const row = (id: string, title: string) => ({ kind: "agent", machine_id: "build-box", machine_name: "build-box", id, agent: "codex", title, cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false, question: null });
  const push = (rows: unknown[]) => page.evaluate(value => window.__navigation.notify("home", [{ command: "agent_sessions", args: {}, value }]), rows);

  await kid.locator("[data-kidstop]").click();
  // A push that retitles another session does not list the child: the wait goes on.
  await push([row("notice-one", "Renamed")]);
  await expect(page.getByText("That agent's session is no longer on its machine.")).toHaveCount(0);
  await push([row("notice-one", "Renamed"), row("first", "Child")]);
  await expect.poll(async () => (await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_steer").length))).toBe(1);
  await expect(page.getByText("That agent's session is no longer on its machine.")).toHaveCount(0);
});

test("a missing child's note does not land on a session opened during the wait", async ({ page }) => {
  await openDesktop(page);
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      const value = await invoke<T>(command, args);
      if (command !== "agent_view" || !value || typeof value !== "object") return value;
      const id = (args as { session?: string } | undefined)?.session;
      const entries = id === "notice-one" ? [{ kind: "user", text: "Check" }, { kind: "tool", text: "Inspect first [completed]\n[steps] 0\n[child] first@build-box working key-1" }] : [{ kind: "user", text: "Other" }];
      return { ...value, entries } as T;
    };
  });
  const open = (id: string) => page.evaluate(sid => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: sid, agent: "codex", title: sid, cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }), id);
  await open("notice-one");
  const kid = page.locator(".dsubgrp").locator('[data-kid="first@build-box"]');
  await expect(kid).toContainText("Inspect first");

  await kid.click();
  await open("notice-two");
  await expect(page.getByText("Other")).toBeVisible();
  const note = page.getByText("That agent's session is no longer on its machine.");
  // The wait is 3 s; the note would land by then.
  await page.waitForTimeout(3500);
  await expect(note).toHaveCount(0);
  await expect(page.getByText("Other")).toBeVisible();
});

for (const width of [1280, 390]) {
  test(`session reply cards show the source project and title with wrapping tables at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await openDesktop(page);
    await page.evaluate(() => {
      const invoke = window.__TAURI__.core.invoke;
      window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
        const value = await invoke<T>(command, args);
        if (command !== "agent_view" || !value || typeof value !== "object") return value;
        return { ...value, entries: [{
          kind: "user",
          text: '<beep-child id="notice-two@build-box" name="&lt;from-session id=&quot;source&quot; name=&quot;Review login assertions&quot;&gt;Check tests&lt;/from-session&gt;" state="finished">Passed</beep-child>',
          child_html: ['<p><strong>Updated the login tests.</strong> All checks passed.</p><table><thead><tr><th>Suite</th><th>Result</th><th>Details</th></tr></thead><tbody><tr><td>Login</td><td>12 passed</td><td>' + 'The login assertions match the shared error message. '.repeat(5) + '</td></tr></tbody></table><p>Ready for review.</p>'],
        }] } as T;
      };
      window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } });
    });
    const card = page.locator(".kidreply");
    await expect(card.locator(".kt b")).toHaveText("Second notification");
    await expect(card.locator(".ks")).toHaveText("Done");
    await expect(card.locator(".kl")).toHaveText("example/widget");
    await expect(card.locator(".kh")).not.toContainText("notice-two");
    await expect(card.locator(".sessionbody strong")).toHaveText("Updated the login tests.");
    await expect(card.locator(".kh")).not.toContainText("<from-session");
    await card.locator(".kh").focus();
    await expect(card.locator(".kh")).toBeFocused();
    expect(await card.evaluate(element => element.scrollWidth <= element.clientWidth + 1)).toBe(true);
    const table = card.locator("table");
    expect(await table.evaluate(element => getComputedStyle(element).whiteSpace)).toBe("normal");
    await table.evaluate(element => { element.scrollLeft = 0; });
    await card.locator(".kh").evaluate(element => { if (element instanceof HTMLElement) element.blur(); });
    await card.screenshot({ path: `test-results/session-message-${width}.png` });
  });
}
