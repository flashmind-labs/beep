import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const session = {
  kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex",
  title: "First notification", cwd: "/srv/project", status: "working", doing: null, question: null,
  started_at: 1, last_activity: 1, external: false,
} as const;

declare global { interface Window { __outbox: { reconnect(): void } } }

/// Opens the session with a stand-in for the app's outbox: while the machine
/// is unreachable, or a message waits before it, a message waits there; on
/// reconnect each goes, in order, and the node holds it as a queued steer.
async function openUnreachable(page: Page) {
  await openDesktop(page, false, { agent_steer: [] });
  await page.evaluate(() => {
    type Row = { id: number; how: string; text: string; images: string[]; state: string; reason: string | null };
    const invoke = window.__TAURI__.core.invoke;
    const where = { machine_id: "build-box", agent: "codex", session: "notice-one" };
    let rows: Row[] = [], next = 0, online = false;
    const changed = (sent: Row[]) => window.__navigation.emit("outbox-changed", { ...where, rows, sent });
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      const steer = args?.steer as { steer?: { text: string } } | undefined;

      if (command === "agent_steer" && steer?.steer && (!online || rows.length)) {
        rows = [...rows, { id: next++, how: "steer", text: steer.steer.text, images: [], state: "waiting", reason: null }];
        changed([]);
        throw { kind: "waiting", message: "Waiting for the connection" };
      }

      if (command === "outbox_list") return rows as T;

      if (command === "outbox_take") {
        const taken = rows.find(row => row.id === args?.id);
        rows = rows.filter(row => row !== taken);
        changed([]);
        return { text: taken?.text ?? "", images: [] } as T;
      }

      return invoke<T>(command, args);
    };
    window.__outbox = {
      reconnect() {
        online = true;
        const sent = rows;
        rows = [];
        const view = { session: { ...where, id: "notice-one", machine_name: "build-box", title: "First notification", cwd: "/srv/project", status: "working", doing: null, question: null, started_at: 1, last_activity: 1, external: false, kind: "agent" }, plan: [], files: [], entries: [], earlier: 0, last_message: "", error: null, turns: 0, queued: sent.length, queued_messages: sent.map(row => row.text) };
        window.__navigation.respond("agent_view", view);
        changed(sent);
      },
    };
  });
  await page.evaluate(row => window.__navigation.open({ agent: row }), session);
  await expect(page.locator("#agmsg")).toBeVisible();
}

async function send(page: Page, text: string) {
  await page.locator("#agmsg").fill(text);
  await page.locator("#agmsg").press("Enter");
}

const waiting = (page: Page) => page.locator(".ag-queue .it.out");
const steers = (page: Page) => page.locator(".ag-queue .it.steer");

test("a message sent while the machine is unreachable waits in the queue and the field stays empty", async ({ page }) => {
  await openUnreachable(page);
  await send(page, "Use the fixture instead");

  await expect(waiting(page)).toHaveCount(1);
  await expect(waiting(page)).toContainText("Use the fixture instead");
  await expect(waiting(page)).toContainText("waiting for connection");
  await expect(page.locator("#agmsg")).toHaveValue("");
});

test("on reconnect the waiting message goes and the node's queued row replaces it", async ({ page }) => {
  await openUnreachable(page);
  await send(page, "Use the fixture instead");
  await expect(waiting(page)).toHaveCount(1);

  await page.evaluate(() => window.__outbox.reconnect());
  await expect(waiting(page)).toHaveCount(0);
  await expect(steers(page)).toHaveCount(1);
  await expect(steers(page)).toContainText("Use the fixture instead");
  await expect(steers(page)).toContainText("after the current step");
});

test("two waiting messages keep the order typed, before and after reconnecting", async ({ page }) => {
  await openUnreachable(page);
  await send(page, "First message");
  await send(page, "Second message");

  await expect(waiting(page).locator(".t")).toHaveText(["First message", "Second message"]);
  await page.evaluate(() => window.__outbox.reconnect());
  await expect(steers(page).locator(".t")).toHaveText(["First message", "Second message"]);
});

test("a waiting message can be removed, or edited back into the field", async ({ page }) => {
  await openUnreachable(page);
  await send(page, "Drop this one");
  await send(page, "Edit this one");
  await expect(waiting(page)).toHaveCount(2);

  await waiting(page).first().getByRole("button", { name: "Remove" }).click();
  await expect(waiting(page).locator(".t")).toHaveText(["Edit this one"]);
  await expect(page.locator("#agmsg")).toHaveValue("");

  await waiting(page).first().getByRole("button", { name: "Edit" }).click();
  await expect(waiting(page)).toHaveCount(0);
  await expect(page.locator("#agmsg")).toHaveValue("Edit this one");
});
