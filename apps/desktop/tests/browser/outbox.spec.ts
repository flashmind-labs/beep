import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const session = {
  kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex",
  title: "First notification", cwd: "/srv/project", status: "working", doing: null, question: null,
  started_at: 1, last_activity: 1, external: false,
} as const;

declare global { interface Window { __outbox: { reconnect(): void; listed(): void; slow(): void; asked: { command: string; seen: unknown }[] } } }

/// Opens the session with a stand-in for the app's outbox: while the machine
/// is unreachable, or a message waits before it, a message waits there; on
/// reconnect each goes, in order, and the node holds it as a queued steer.
async function openUnreachable(page: Page) {
  await openDesktop(page, false, { agent_steer: [] });
  // The session has no follow here, so its page is read again when the follow is tried again.
  await page.clock.install();
  await page.evaluate(() => {
    type Row = { id: number; how: string; text: string; images: string[]; state: string; reason: string | null; keys: string[] };
    const invoke = window.__TAURI__.core.invoke;
    const where = { machine_id: "build-box", agent: "codex", session: "notice-one" };
    let rows: Row[] = [], next = 0, online = false, slow = false, revision = 0;
    const asked: { command: string; seen: unknown }[] = [];
    const changed = (sent: Row[]) => window.__navigation.emit("outbox-changed", { ...where, revision: ++revision, rows, sent });
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      const steer = args?.steer as { steer?: { text: string } } | undefined;
      if (command.startsWith("outbox_") || command === "agent_steer") asked.push({ command, seen: args?.seen ?? null });

      if (command === "agent_steer" && steer?.steer && (slow || !online || rows.length)) {
        rows = [...rows, { id: next++, how: "steer", text: steer.steer.text, images: [], state: slow ? "sending" : "waiting", reason: null, keys: [] }];
        changed([]);
        // A first try that outlasts the page's wait: the outbox shows it, the page gives up.
        if (slow) throw "no answer in 40 s. Try again.";
        throw { kind: "waiting", message: "Waiting for the connection" };
      }

      if (command === "outbox_drain") {
        const taken = rows.filter(row => row.state !== "sending");
        rows = rows.filter(row => row.state === "sending");
        changed([]);
        return taken.map(row => ({ text: row.text, images: [], files: [], mentions: [] })) as T;
      }

      if (command === "outbox_list") {
        return { revision, session: where.session, rows } as T;
      }

      if (command === "outbox_take") {
        const taken = rows.find(row => row.id === args?.id);
        rows = rows.filter(row => row !== taken);
        changed([]);
        return { text: taken?.text ?? "", images: [], files: [], mentions: [] } as T;
      }

      return invoke<T>(command, args);
    };
    window.__outbox = {
      asked,
      slow() { slow = true; online = true; },
      reconnect() {
        online = true;
        // The machine takes each: its rows stay, as sent, until the session lists them.
        const sent = rows.map(row => ({ ...row, state: "sent" }));
        rows = sent;
        changed(sent);
      },
      listed() {
        const sent = rows;
        rows = [];
        const view = { session: { ...where, id: "notice-one", machine_name: "build-box", title: "First notification", cwd: "/srv/project", status: "working", doing: null, question: null, started_at: 1, last_activity: 1, external: false, kind: "agent" }, plan: [], entries: [], earlier: 0, error: null, turns: 0, queued: sent.length, queued_messages: sent.map(row => row.text) };
        window.__navigation.respond("agent_view", view);
        changed([]);
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
  // What the page saw goes with it, so a check after a dropped send counts only what came after.
  const sent = await page.evaluate(() => window.__outbox.asked.filter(call => call.command === "agent_steer").map(call => call.seen));
  expect(sent).toEqual([{ entries: 0, queued: [] }]);
});

test("a message whose send got no answer in time stays out of the field and shows in the queue", async ({ page }) => {
  await openUnreachable(page);
  await page.evaluate(() => window.__outbox.slow());
  await send(page, "Slow one");

  await expect(waiting(page)).toHaveCount(1);
  await expect(waiting(page)).toContainText("Slow one");
  await expect(waiting(page)).toContainText("sending");
  await expect(page.locator("#agmsg")).toHaveValue("");
});

test("a late sending event cannot put a delivered image prompt back in the queue", async ({ page }) => {
  await openUnreachable(page);
  await page.evaluate(row => window.__navigation.respond("agent_view", {
    session: row, plan: [], earlier: 0, error: null, turns: 1,
    queued: 0, queued_messages: [], entries: [
      { kind: "user", text: "Check the diagram\n[image: diagram.png]" },
      { kind: "agent", text: "The diagram is ready." },
    ],
  }), session);
  await page.clock.runFor(5000);
  await expect(page.getByText("The diagram is ready.", { exact: true })).toBeVisible();
  // The delivery was pushed first, as revision 2.
  await page.evaluate(() => window.__navigation.emit("outbox-changed", { machine_id: "build-box", agent: "codex", session: "notice-one", revision: 2, rows: [], sent: [] }));
  await page.evaluate(() => {
    window.__navigation.emit("outbox-changed", {
      machine_id: "build-box", agent: "codex", session: "notice-one",
      revision: 1,
      rows: [{ id: 0, how: "steer", text: "Check the diagram", images: ["diagram.png"], state: "sending", reason: null }],
      sent: [],
    });
  });
  await expect(page.locator("#agqueue")).toHaveCount(0);
});

test("the rows an event carries are drawn as they come, with no read, and an older set never replaces a newer one", async ({ page }) => {
  await openUnreachable(page);
  const row = (id: number, text: string) => ({ id, how: "steer", text, images: [], state: "uploading", reason: null });
  const push = (revision: number, rows: unknown[]) => page.evaluate(({ revision, rows }) => window.__navigation.emit("outbox-changed", { machine_id: "build-box", agent: "codex", session: "notice-one", revision, rows, sent: [], resumed: null, uploaded: [] }), { revision, rows });
  const reads = () => page.evaluate(() => window.__outbox.asked.filter(call => call.command === "outbox_list").length);

  await push(5, [row(0, "Read these")]);
  await expect(page.locator(".ag-queue .it.out")).toHaveCount(1);
  await expect(page.locator(".ag-queue .it.out")).toContainText("uploading");
  // Reordered in flight: the older set arrives after the newer one.
  await push(4, []);
  await expect(page.locator(".ag-queue .it.out")).toHaveCount(1);
  await push(6, []);
  await expect(page.locator(".ag-queue .it.out")).toHaveCount(0);
  expect(await reads()).toBe(1);
});

test("a page opened on a session that was resumed goes on in the one that runs, and draws no rows of it on the ended one", async ({ page }) => {
  await openDesktop(page, false, {});
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => command === "outbox_list" && args?.session === "notice-one"
      ? { revision: 3, session: "notice-two", rows: [{ id: 0, how: "steer", text: "Read these", images: [], state: "uploading", reason: null }] } as T
      : invoke<T>(command, args);
  });
  await page.evaluate(row => window.__navigation.open({ agent: row }), session);
  await expect(page.locator("#agmsg")).toBeVisible();
  await expect(page.locator(".ag-queue .it.out")).toHaveCount(0);
  // The page opened on the ended session went on in the one that runs.
  const followed = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_follow").map(call => String(call.args.session)));
  await expect.poll(followed).toContain("notice-two");
});

test("Stop all takes every waiting message back into the field at once, oldest first", async ({ page }) => {
  await openUnreachable(page);
  await send(page, "First message");
  await send(page, "Second message");
  await expect(waiting(page)).toHaveCount(2);

  await page.evaluate(() => window.__navigation.open("home"));
  await page.locator(".cwin").getByText("First notification").first().click({ button: "right" });
  await page.getByRole("menuitem", { name: "Stop all" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_steer").map(call => call.args.steer))).toContain("stop_all");
  const drains = await page.evaluate(() => window.__outbox.asked.filter(call => ["outbox_drain", "outbox_take"].includes(call.command)).map(call => call.command));
  expect(drains).toEqual(["outbox_drain"]);

  await page.evaluate(row => window.__navigation.open({ agent: row }), session);
  await expect(page.locator("#agmsg")).toHaveValue("First message\nSecond message");
  await expect(waiting(page)).toHaveCount(0);
});

test("on reconnect the message stays as sent until the node lists it, and its queued row replaces it", async ({ page }) => {
  await openUnreachable(page);
  await send(page, "Use the fixture instead");
  await expect(waiting(page)).toHaveCount(1);

  await page.evaluate(() => window.__outbox.reconnect());
  await expect(waiting(page)).toHaveCount(1);
  await expect(waiting(page)).toContainText("after the current step");
  await expect(waiting(page).getByRole("button", { name: "Edit" })).toBeDisabled();
  await expect(steers(page)).toHaveCount(0);

  await page.evaluate(() => window.__outbox.listed());
  await page.clock.runFor(5000);
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
  await expect(waiting(page).locator(".t")).toHaveText(["First message", "Second message"]);
  await page.evaluate(() => window.__outbox.listed());
  await page.clock.runFor(5000);
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
