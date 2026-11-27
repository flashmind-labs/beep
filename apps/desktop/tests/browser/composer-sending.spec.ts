import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const session = { kind: "agent" as const, machine_id: "build-box", machine_name: "build-box", id: "send-one", agent: "codex", title: "Sending", cwd: "/srv/project", status: "idle", doing: null, started_at: 1, last_activity: 1, external: false };
const uploaded = "/srv/project/.beep/uploads/notes.txt";
const tiny = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
const image = { name: "shot.png", mimeType: "image/png", buffer: Buffer.from(tiny, "base64") };

async function open(page: Page, status = "idle", responses: Record<string, unknown> = {}) {
  const row = { ...session, status };
  await openDesktop(page, false, {
    agent_view: { session: row, model: null, plan: [], earlier: 0, error: null, turns: 0, queued: 0, queued_messages: [], entries: [] },
    agent_steer: [], agent_tree_upload: uploaded, ...responses,
  });
  await page.evaluate(agent => window.__navigation.open({ agent }), row);
}

const calls = (page: Page, command: string) => page.evaluate(name => window.__navigation.calls().filter(call => call.command === name), command);
const picker = (page: Page) => page.locator('#agfield input[type="file"]');

const where = { machine_id: "build-box", agent: "codex", session: "send-one" };
const notes = { name: "notes.txt", mimeType: "text/plain", buffer: Buffer.from("notes") };

/// Stands in for the app's outbox: a message with files is kept as a row that
/// says "uploading", and `agent_steer` answers "waiting" at once, as the app does.
async function keepFilesInOutbox(page: Page) {
  await page.evaluate(where => {
    type Row = { id: number; how: string; text: string; images: string[]; state: string; reason: string | null };
    const invoke = window.__TAURI__.core.invoke;
    let rows: Row[] = [], next = 0, revision = 0;
    const asked: { args: unknown }[] = [], order: string[] = [], attached: unknown[] = [];
    Object.assign(window, { __asked: asked, __order: order, __attached: attached });
    const changed = () => window.__navigation.emit("outbox-changed", { ...where, revision: ++revision, rows, sent: [] });
    Object.assign(window, { __rows: { set(update: (rows: Row[]) => Row[]) { rows = update(rows); changed(); } } });
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      const steer = args?.steer as { steer?: { text: string } } | undefined;

      if (command === "agent_steer" && steer?.steer) order.push(steer.steer.text);

      if (command === "agent_steer" && (args?.attach || args?.resume) && steer?.steer) {
        asked.push({ args });
        rows = [...rows, { id: next++, how: "steer", text: steer.steer.text, images: [], state: args.resume ? "resuming" : (args.attach as { reading?: string }).reading ? "reading" : "uploading", reason: null }];
        changed();
        throw { kind: "waiting", message: "Waiting for the connection" };
      }

      if (command === "outbox_attach") {
        attached.push(args);
        rows = rows.map(row => row.state === "reading" ? { ...row, state: "uploading" } : row);
        changed();
        return true as T;
      }

      if (command === "outbox_list") return { revision, rows } as T;

      if (command === "outbox_take") {
        const taken = rows.find(row => row.id === args?.id);
        rows = rows.filter(row => row !== taken);
        changed();
        return { text: taken?.text ?? "", images: [], files: [{ name: "notes.txt", mime: "text/plain", data: "bm90ZXM=" }], mentions: [] } as T;
      }

      return invoke<T>(command, args);
    };
  }, where);
}

declare global { interface Window { __rows: { set(update: (rows: { id: number; how: string; text: string; images: string[]; state: string; reason: string | null }[]) => { id: number; how: string; text: string; images: string[]; state: string; reason: string | null }[]): void } } }

test("a message with files shows at once as uploading and the field stays usable", async ({ page }) => {
  await open(page);
  await keepFilesInOutbox(page);
  await picker(page).setInputFiles([notes]);
  await expect(page.locator("#agfield .attachment-count")).toHaveText("1 file");
  await page.locator("#agmsg").fill("Read the notes");
  await page.locator("#agmsg").press("Enter");

  const out = page.locator(".ag-queue .it.out");
  await expect(out).toHaveCount(1);
  await expect(out.first()).toContainText("Read the notes");
  await expect(out.first()).toContainText("uploading");
  await expect(out.first().locator(".dspin")).toBeVisible();
  await expect(out.first().locator('[data-act="edit"]')).toBeDisabled();
  await expect(page.locator("#agmsg")).toHaveValue("");
  await expect(page.locator("#agmsg")).toBeEditable();
  await expect(page.locator("#agsend")).toBeEnabled();

  // The window hands over the files with the words, and uploads nothing itself.
  const sent = await page.evaluate(() => (window as unknown as { __asked: { args: unknown }[] }).__asked);
  expect(sent).toHaveLength(1);
  expect(sent[0].args).toMatchObject({ steer: { steer: { text: "Read the notes" } }, attach: { files: [{ name: "notes.txt", mime: "text/plain", data: "bm90ZXM=" }], mentions: [] } });
  expect(await calls(page, "agent_tree_upload")).toEqual([]);

  // The outbox moves it on: its row says sending, then it is gone.
  await page.evaluate(() => window.__rows.set(rows => rows.map(row => ({ ...row, state: "sending" }))));
  await expect(out.first()).toContainText("sending");
  await page.evaluate(() => window.__rows.set(() => []));
  await expect(out).toHaveCount(0);
});

test("a message whose upload failed stays as a failed row, and Edit puts its words and files back", async ({ page }) => {
  await open(page);
  await keepFilesInOutbox(page);
  await picker(page).setInputFiles([notes]);
  await page.locator("#agmsg").fill("Read the notes");
  await page.locator("#agmsg").press("Enter");
  const out = page.locator(".ag-queue .it.out");
  await expect(out).toHaveCount(1);

  await page.evaluate(() => window.__rows.set(rows => rows.map(row => ({ ...row, state: "failed", reason: "no space left" }))));
  await expect(out.first()).toContainText("no space left");
  await out.first().locator('[data-act="edit"]').click();

  await expect(out).toHaveCount(0);
  await expect(page.locator("#agmsg")).toHaveValue("Read the notes");
  await expect(page.locator("#agfield .attachment-count")).toHaveText("1 file");
  expect(await calls(page, "agent_tree_upload")).toEqual([]);
});

test("Enter pressed again while a dropped file is read sends no junk message, and the messages go in the order of Enter", async ({ page }) => {
  await open(page);
  await keepFilesInOutbox(page);
  // The file's read waits until the test lets it finish.
  await page.evaluate(() => {
    const read = FileReader.prototype.readAsDataURL;
    let release = () => {};
    const gate = new Promise<void>(resolve => { release = resolve; });
    Object.assign(window, { __releaseRead: release });
    FileReader.prototype.readAsDataURL = function (this: FileReader, blob: Blob) { void gate.then(() => read.call(this, blob)); };
  });
  await picker(page).setInputFiles([notes]);
  await page.locator("#agmsg").fill("First");
  await page.locator("#agmsg").press("Enter");
  // Nothing typed, the read already belongs to the first message: no message.
  await page.locator("#agmsg").press("Enter");
  await page.locator("#agmsg").fill("Second");
  await page.locator("#agmsg").press("Enter");
  // The first is the outbox's from Enter, with its words, while its file is read.
  await expect(page.locator(".ag-queue .it.out")).toHaveCount(1);
  await expect(page.locator(".ag-queue .it.out")).toContainText("reading files");
  await expect.poll(async () => (await calls(page, "agent_steer")).length).toBe(1);
  expect(await page.evaluate(() => (window as unknown as { __attached: unknown[] }).__attached)).toEqual([]);

  await page.evaluate(() => (window as unknown as { __releaseRead(): void }).__releaseRead());
  await expect(page.locator(".ag-queue .it.out")).toContainText("uploading");
  expect(await page.evaluate(() => (window as unknown as { __attached: unknown[] }).__attached)).toHaveLength(1);
  const withFile = await page.evaluate(() => (window as unknown as { __asked: { args: { steer: { steer: { text: string } } } }[] }).__asked);
  expect(withFile.map(call => call.args.steer.steer.text)).toEqual(["First"]);
  // The first reached the app before the second, which has no file.
  const second = (await calls(page, "agent_steer"))[0];
  expect(JSON.stringify(second.args)).toContain("Second");
  expect(JSON.stringify([withFile, second])).not.toContain("attachment:");
  const order = await page.evaluate(() => (window as unknown as { __order: string[] }).__order);
  expect(order).toEqual(["First", "Second"]);
});

test("an ended session hands the message and its resume to the outbox, shows a row, and opens the session that resumed", async ({ page }) => {
  const resumed = { ...session, id: "send-two", status: "idle" };
  await open(page, "ended");
  await keepFilesInOutbox(page);
  await picker(page).setInputFiles([image]);
  await page.locator("#agmsg").fill("Carry on");
  await page.locator("#agmsg").press("Enter");

  const out = page.locator(".ag-queue .it.out");
  await expect(out).toHaveCount(1);
  await expect(out.first()).toContainText("Carry on");
  await expect(out.first()).toContainText("resuming");
  await expect(page.locator("#agmsg")).toBeEditable();
  await expect(page.locator("#agmsg")).toHaveValue("");

  // A second message while it resumes queues behind the first: the window asks to resume again, the app does it once.
  await page.locator("#agmsg").fill("And this");
  await page.locator("#agmsg").press("Enter");
  await expect(out).toHaveCount(2);

  const sent = await page.evaluate(() => (window as unknown as { __asked: { args: { resume?: unknown; steer: unknown } }[] }).__asked);
  expect(sent).toHaveLength(2);
  expect(JSON.stringify(sent[0].args.steer)).toContain(tiny);
  expect(sent[0].args.resume).toEqual({ model: null, pick: null });
  expect(await calls(page, "agent_resume")).toEqual([]);

  // The app resumed it: the window opens the session that goes on, in the background.
  await page.evaluate(resumed => window.__navigation.respond("agent_view", { session: resumed, model: null, plan: [], earlier: 0, error: null, turns: 0, queued: 0, queued_messages: [], entries: [] }), resumed);
  await page.evaluate(({ where, resumed }) => window.__navigation.emit("outbox-changed", { ...where, revision: 1000, rows: [], sent: [], resumed, uploaded: [] }), { where, resumed });
  await expect.poll(async () => (await calls(page, "agent_view")).some(call => (call.args as { session?: string }).session === "send-two")).toBe(true);
});

test("resuming an ended session under the same id clears its error card and follows the new turn without stalling", async ({ page }) => {
  const endedRow = { ...session, status: "ended" };
  const endedPage = {
    session: endedRow, model: null, plan: [], earlier: 0, error: "the agent exited",
    turns: 1, queued: 0, queued_messages: [], entries: [{ kind: "agent", text: "Before exit" }],
  };
  await openDesktop(page, false, { agent_view: endedPage, agent_steer: [], agent_tree_upload: uploaded });
  await page.evaluate(({ where, endedPage, session }) => {
    const invoke = window.__TAURI__.core.invoke;
    let live = false;
    let revivedOnView = false;
    let follows = 0;
    let lastChannel: { onmessage(message: unknown): void } | null = null;
    let closeEnded: (() => void) | null = null;
    const cursor = { start: 0, next: 1, fingerprint: "ended-1" };
    Object.assign(window, {
      __follows: () => follows,
      __triggerRace: () => {
        window.__navigation.hold("agent_view");
        // Queue a stale ended frame for `requestAnimationFrame`, then close the ended stream so
        // `disconnected` starts `pollAgent(false)` (`AG.inflight`) before that frame runs.
        lastChannel?.onmessage({ kind: "snapshot", resumed: true, cursor, page: endedPage, modes: null, tasks: [] });
        revivedOnView = true;
        closeEnded?.();
      },
    });
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command === "agent_follow") {
        follows++;
        const ch = args?.onEvent as { onmessage(message: unknown): void } | undefined;
        if (ch) lastChannel = ch;
        if (!live) {
          ch?.onmessage({ kind: "snapshot", resumed: false, cursor, page: endedPage, modes: null, tasks: [] });
          return new Promise<never>((_, reject) => {
            closeEnded = () => reject(new Error("session stream disconnected"));
          });
        }
        ch?.onmessage({
          kind: "snapshot", resumed: false,
          cursor: { start: 0, next: 3, fingerprint: "live-3" },
          page: {
            session: { ...session, status: "working" }, model: null, plan: [], earlier: 0, error: null,
            turns: 2, queued: 0, queued_messages: [],
            entries: [{ kind: "agent", text: "Before exit" }, { kind: "user", text: "Carry on" }, { kind: "agent", text: "Back to work" }],
          },
          modes: null, tasks: [],
        });
        return new Promise<never>(() => {});
      }
      if (command === "agent_view" && revivedOnView) {
        revivedOnView = false;
        live = true;
        // `outbox-changed` arrives while `agent_view` is in flight and the stale rAF frame is still pending.
        const resumed = { ...session, status: "starting" };
        window.__navigation.emit("outbox-changed", { ...where, revision: 2, rows: [], sent: [], resumed, pick: null });
      }
      if (command === "agent_steer" && args?.resume) {
        window.__navigation.emit("outbox-changed", {
          ...where, revision: 1,
          rows: [{ id: 1, how: "steer", text: "Carry on", images: [], state: "resuming", reason: null, keys: [] }],
          sent: [], resumed: null, pick: null,
        });
        throw { kind: "waiting", message: "Waiting for the connection" };
      }
      return invoke<T>(command, args);
    };
  }, { where, endedPage, session });

  await page.evaluate(agent => window.__navigation.open({ agent }), endedRow);
  await expect(page.locator(".derrcard")).toContainText("the agent exited");
  expect(await page.evaluate(() => (window as unknown as { __follows(): number }).__follows())).toBe(1);

  // Sending while ended hands the resume to the outbox without opening a second follow against the ended session.
  await page.locator("#agmsg").fill("Carry on");
  await page.locator("#agmsg").press("Enter");
  await expect(page.locator(".ag-queue .it.out")).toContainText("resuming");
  expect(await page.evaluate(() => (window as unknown as { __follows(): number }).__follows())).toBe(1);

  await page.evaluate(() => (window as unknown as { __triggerRace(): void }).__triggerRace());

  // The old error card clears immediately and stays gone across animation frames while `agent_view` is still held.
  await expect(page.locator(".derrcard")).toHaveCount(0);
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  await expect(page.locator(".derrcard")).toHaveCount(0);

  // Releasing the stale `agent_view` immediately opens the new follow without waiting for the 5 s retry timer.
  await page.evaluate(() => window.__navigation.release("agent_view"));
  await expect(page.locator(".dcoderen.agent").last()).toContainText("Back to work", { timeout: 2000 });
});

test("a pasted image crosses once, with the message, and is not uploaded", async ({ page }) => {
  await open(page);
  await picker(page).setInputFiles([image]);
  await expect(page.locator("#agfield .attachment-count")).toHaveText("1 file");
  await page.locator("#agmsg").fill("Look");
  await page.locator("#agmsg").press("Enter");
  await expect.poll(async () => (await calls(page, "agent_steer")).length).toBe(1);

  const all = await page.evaluate(() => window.__navigation.calls().map(call => ({ command: call.command, text: JSON.stringify(call.args) })));
  expect(all.filter(call => call.text.includes(tiny)).map(call => call.command)).toEqual(["agent_steer"]);
  expect(all.filter(call => call.command === "agent_tree_upload")).toEqual([]);
  await expect(page.locator(".dcoderen.user").first()).toContainText("Look");
});

