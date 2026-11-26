import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("session links open the target session directly, beside on modifier click, and never invoke open_url", async ({ page }) => {
  await openDesktop(page);

  const archived = {
    machine_id: "build-box",
    machine_name: "build-box",
    id: "session_archived_01",
    agent: "flsh",
    title: "Archived migration session",
    cwd: "/srv/project",
    status: "ended",
    doing: null,
    started_at: 10,
    last_activity: 20,
    external: true,
    question: null,
    assistant: false,
    persona: null,
  };

  await page.evaluate(target => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command === "agent_session_find") {
        if (args?.session === target.id) return target as T;
        return null as T;
      }

      const value = await invoke<T>(command, args);
      if (command !== "agent_view" || !value || typeof value !== "object") return value;

      if (args?.session === target.id) {
        return {
          ...value,
          session: target,
          entries: [{ kind: "user", text: "Migrate" }, { kind: "agent", text: "Migration complete.", html: "<p>Migration complete.</p>" }],
        } as T;
      }

      return {
        ...value,
        entries: [
          { kind: "user", text: "Which session?" },
          {
            kind: "agent",
            text: `See [Archived migration](beep://session/${target.id}@build-box).`,
            html: `<p>See <a href="beep://session/${target.id}@build-box" rel="noopener noreferrer">Archived migration</a>.</p>`,
          },
        ],
      } as T;
    };
  }, archived);

  await page.evaluate(() => window.__navigation.open({
    agent: {
      kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex",
      title: "Example session", cwd: "/srv/project", status: "idle", doing: null, started_at: 1, last_activity: 1, external: false,
    },
  }));

  const link = page.locator('.dcoderen.agent a[href="beep://session/session_archived_01@build-box"]');
  await expect(link).toBeVisible();

  // Right-click menu offers session actions.
  await link.click({ button: "right" });
  await expect(page.getByRole("menuitem", { name: "Open session" })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "Open beside" })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "Copy link" })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "Copy id" })).toBeVisible();
  await page.keyboard.press("Escape");

  // Modifier click opens the linked session beside in a split tab.
  const modifier = await page.evaluate(() => /Mac/.test(navigator.userAgent) ? "Meta" : "Control");
  await link.click({ modifiers: [modifier] });
  await expect(page.locator(".agsplit-head")).toContainText("Archived migration session");

  // Plain click opens the archived session directly via agent_session_find.
  await link.click();
  await expect(page.locator(".dcoderen.agent")).toContainText("Migration complete.");

  // Clicking beep://session/... must never delegate to external open_url.
  const openedUrls = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "open_url"));
  expect(openedUrls).toEqual([]);
});

test("inline code matching another known session opens it on click", async ({ page }) => {
  await openDesktop(page);

  const known = {
    machine_id: "build-box", machine_name: "build-box", id: "known-target-99", agent: "pi",
    title: "Known target session", cwd: "/srv/project", status: "idle", doing: null,
    started_at: 5, last_activity: 15, external: false, question: null, assistant: false, persona: null,
  };

  await page.evaluate(target => {
    window.__navigation.notify("home", [{ command: "agent_sessions", value: [target] }]);
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      const value = await invoke<T>(command, args);
      if (command !== "agent_view" || !value || typeof value !== "object") return value;
      if (args?.session === target.id) {
        return {
          ...value,
          session: target,
          entries: [{ kind: "agent", text: "Inside known target.", html: "<p>Inside known target.</p>" }],
        } as T;
      }
      return {
        ...value,
        entries: [{ kind: "agent", text: `Check \`${target.id}\`.`, html: `<p>Check <code>${target.id}</code>.</p>` }],
      } as T;
    };
  }, known);

  await page.evaluate(() => window.__navigation.open({
    agent: {
      kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex",
      title: "Example session", cwd: "/srv/project", status: "idle", doing: null, started_at: 1, last_activity: 1, external: false,
    },
  }));

  const ref = page.locator('code.fref[data-session="known-target-99"]');
  await expect(ref).toBeVisible();
  await ref.click();
  await expect(page.locator(".dcoderen.agent")).toContainText("Inside known target.");
});
