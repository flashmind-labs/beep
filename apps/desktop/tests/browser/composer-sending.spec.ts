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
const rows = (page: Page) => page.locator(".ag-queue .it.steer");

test("a message with an upload in progress shows at once and the field stays usable", async ({ page }) => {
  await open(page);
  await page.evaluate(() => window.__navigation.hold("agent_tree_upload"));
  await picker(page).setInputFiles([{ name: "notes.txt", mimeType: "text/plain", buffer: Buffer.from("notes") }]);
  await page.locator("#agmsg").fill("Read the notes");
  await page.locator("#agmsg").press("Enter");

  await expect(rows(page)).toHaveCount(1);
  await expect(rows(page).first()).toContainText("Read the notes");
  await expect(rows(page).first()).toContainText("uploading");
  await expect(rows(page).first().locator(".dspin")).toBeVisible();
  await expect(page.locator("#agmsg")).toHaveValue("");
  await expect(page.locator("#agmsg")).toBeEditable();
  await expect(page.locator("#agsend")).toBeEnabled();
  expect(await calls(page, "agent_steer")).toEqual([]);

  // A follow-up typed meanwhile goes behind the first.
  await page.locator("#agmsg").fill("Then this");
  await page.locator("#agmsg").press("Enter");
  await expect(rows(page)).toHaveCount(2);
  await expect(rows(page).nth(1)).toContainText("waiting");
  expect(await calls(page, "agent_steer")).toEqual([]);

  await page.evaluate(() => window.__navigation.release("agent_tree_upload"));
  await expect.poll(async () => (await calls(page, "agent_steer")).length).toBe(2);
  const sent = (await calls(page, "agent_steer")).map(call => JSON.stringify(call.args.steer));
  expect(sent[0]).toContain("Read the notes");
  expect(sent[0]).toContain(uploaded);
  expect(sent[1]).toContain("Then this");
  await expect(rows(page)).toHaveCount(0);
});

test("a failed upload puts the message back in the field with the error", async ({ page }) => {
  await open(page);
  await page.evaluate(() => window.__navigation.hold("agent_tree_upload"));
  await picker(page).setInputFiles([{ name: "notes.txt", mimeType: "text/plain", buffer: Buffer.from("notes") }]);
  await page.locator("#agmsg").fill("Read the notes");
  await page.locator("#agmsg").press("Enter");
  await expect(rows(page)).toHaveCount(1);

  await page.evaluate(() => { window.__navigation.rejectOnce("agent_tree_upload", "no space left"); window.__navigation.release("agent_tree_upload"); });
  await expect(rows(page)).toHaveCount(0);
  await expect(page.locator("#agmsg")).toHaveValue("Read the notes");
  await expect(page.locator("span.derr")).toContainText("no space left");
  expect(await calls(page, "agent_steer")).toEqual([]);
});

test("an ended session shows the message at once and keeps the field usable while it resumes", async ({ page }) => {
  const resumed = { ...session, status: "idle" };
  await open(page, "ended", { agent_resume: { session: resumed, why: "" } });
  await page.evaluate(() => window.__navigation.hold("agent_resume"));
  await picker(page).setInputFiles([image]);
  await page.locator("#agmsg").fill("Carry on");
  await page.locator("#agmsg").press("Enter");

  await expect(page.locator(".dcoderen.user").first()).toContainText("Carry on");
  await expect(page.locator("#agmsg")).toBeEditable();
  await expect(page.locator("#agmsg")).toHaveValue("");
  expect(await calls(page, "agent_steer")).toEqual([]);

  await page.evaluate(() => window.__navigation.release("agent_resume"));
  await expect.poll(async () => (await calls(page, "agent_steer")).length).toBe(1);
  expect(JSON.stringify((await calls(page, "agent_steer"))[0].args)).toContain(tiny);
});
