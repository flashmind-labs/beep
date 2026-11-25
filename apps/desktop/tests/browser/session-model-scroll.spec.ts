import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const session = {
  machine_id: "build-box", machine_name: "build-box", id: "imported-session", agent: "codex",
  title: "Imported conversation", cwd: "/srv/project", status: "ended", external: true,
  doing: null, question: null, started_at: 1, last_activity: 1,
};
const view = (row = session) => ({
  session: row, model: null, plan: [], entries: Array.from({ length: 40 }, (_, i) => ({ kind: i % 2 ? "assistant" : "user", text: `Message ${i}\n${"Conversation text. ".repeat(30)}` })),
  earlier: 0, error: null, queued: 0, queued_messages: [], turns: 20,
});

test("opening a short transcript page reads all earlier messages without scrolling", async ({ page }) => {
  const active = { ...session, status: "idle", external: false };
  const entries = [{ kind: "other", text: "[conversation compacted]\nEarlier context." }, { kind: "agent", text: "Latest reply" }];
  await openDesktop(page, false, { agent_view: { ...view(active), entries, earlier: 202 } });
  await page.evaluate(() => {
    window.__navigation.holdNext("agent_older");
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = <T>(command: string, args?: Record<string, unknown>) => {
      if (command === "agent_older") {
        const batch = args?.before === 202
          ? { earlier: 2, entries: Array.from({ length: 200 }, () => ({ kind: "thought", text: "Thinking" })) }
          : { earlier: 0, entries: [{ kind: "user", text: "First prompt" }, { kind: "agent", text: "First reply" }] };
        window.__navigation.respond(command, batch);
      }
      return invoke<T>(command, args);
    };
  });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), active);
  const body = page.locator("#agbody");
  const reads = () => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_older").map(c => c.args.before));
  await expect(body).toContainText("Latest reply");
  expect(await body.evaluate(b => b.scrollHeight === b.clientHeight)).toBe(true);
  await expect.poll(reads).toEqual([202]);

  await page.evaluate(() => window.__navigation.release("agent_older"));
  await expect.poll(reads).toEqual([202, 2]);
  await expect(body).toContainText("First prompt");
  await body.getByText("Context summarized", { exact: true }).click();
  await expect(body.getByText("Earlier context.", { exact: true })).toBeVisible();
  await expect(body).toContainText("Latest reply");
});

test("an imported session with an unknown model can choose before continuing", async ({ page }) => {
  const resumed = { ...session, id: "resumed-session", external: false, status: "idle" };
  await openDesktop(page, false, {
    agent_view: view(), agent_resume: { session: resumed, why: "" }, agent_steer: [],
    agent_models: { pick: "listed", current: null, models: [{ id: "test-model", name: "Test model", description: null }] },
  });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), session);
  await expect(page.locator("#agmodelb")).toBeEnabled();
  await expect(page.locator("#agmodelb")).toContainText("Model unknown");
  await page.locator("#agmodelb").click();
  await page.locator('[data-agmo="test-model"]').click();
  await expect(page.locator("#agmodelb")).toContainText("Test model");
  expect(await page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_steer"))).toEqual([]);
  await page.locator("#agmsg").fill("Continue with this model");
  await page.locator("#agmsg").press("Enter");
  // One call hands the message and the resume to the app; the window resumes nothing itself.
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_steer").length)).toBe(1);
  const calls = await page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_resume" || c.command === "agent_steer"));
  expect(calls.map(c => c.command)).toEqual(["agent_steer"]);
  expect(calls[0].args).toMatchObject({ session: session.id, steer: { steer: { text: "Continue with this model" } }, resume: { model: "test-model", pick: { model: "test-model", effort: null, fast: null } } });
  await page.evaluate(({ session, resumed }) => window.__navigation.emit("outbox-changed", { machine_id: session.machine_id, agent: session.agent, session: session.id, revision: 1, rows: [], sent: [], resumed, uploaded: [] }), { session, resumed });
  await expect(page.locator("#agmodelb")).toContainText("Test model");
});

test("a resumed session takes the model sent with the message, not one picked after Enter", async ({ page }) => {
  const resumed = { ...session, id: "resumed-session", external: false, status: "idle" };
  const models = [{ id: "test-model", name: "Test model", description: null }, { id: "later-model", name: "Later model", description: null }];
  await openDesktop(page, false, { agent_view: view(), agent_resume: { session: resumed, why: "" }, agent_steer: [], agent_models: { pick: "listed", current: null, models } });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), session);
  await page.locator("#agmodelb").click();
  await page.locator('[data-agmo="test-model"]').click();
  await page.locator("#agmsg").fill("Continue with this model");
  await page.locator("#agmsg").press("Enter");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_steer").length)).toBe(1);

  // Another pick before the app says it resumed the session does not change what was sent.
  await page.locator("#agmodelb").click();
  await page.locator('[data-agmo="later-model"]').click();
  await expect(page.locator("#agmodelb")).toContainText("Later model");
  await page.evaluate(({ session, resumed, next }) => {
    window.__navigation.respond("agent_view", next);
    window.__navigation.emit("outbox-changed", { machine_id: session.machine_id, agent: session.agent, session: session.id, revision: 1, rows: [], sent: [], resumed, uploaded: [] });
  }, { session, resumed, next: view(resumed) });
  await expect(page.locator("#agmodelb")).toContainText("Test model");
});

test("a failed turn runs its prompt again on the model picked from its error", async ({ page }) => {
  const idle = { ...session, agent: "claude", status: "idle", external: false };
  const entries = [{ kind: "user", text: "<beep-context>\nContext\n</beep-context>\nCheck the alerts" }];
  await openDesktop(page, false, {
    agent_view: { ...view(idle), entries, error: "No endpoints found for this model" }, agent_steer: [],
    agent_models: { pick: "listed", current: null, models: [{ id: "test-model", name: "Test model", description: null }] },
  });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), idle);
  await page.getByRole("button", { name: "Retry with another model" }).click();
  await page.locator('[data-agmo="test-model"]').click();

  const steers = () => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_steer").map(c => c.args.steer));
  await expect.poll(steers).toEqual([
    { configure: { model: "test-model", effort: null, fast: null } },
    { prompt: { text: "Check the alerts" } },
  ]);
});

test("touch scrolling releases the log before the next animation frame", async ({ page }) => {
  const active = { ...session, status: "working", external: false };
  await openDesktop(page, false, { agent_view: view(active) });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), active);
  const body = page.locator("#agbody");
  const details = page.locator("#agshortcuts"), summary = details.locator("summary");
  await summary.click();
  await summary.focus();
  await details.evaluate(el => Reflect.set(window, "sessionHintsDisclosure", el));
  await expect(body).toContainText("Message 39");
  await expect.poll(() => body.evaluate(el => el.scrollHeight > el.clientHeight)).toBe(true);
  const state = await body.evaluate((el: HTMLElement & { stuck?: boolean; touching?: boolean }) => {
    el.scrollTop = el.scrollHeight;
    el.dispatchEvent(new Event("scroll"));
    el.dispatchEvent(new Event("touchstart"));
    el.dispatchEvent(new Event("touchmove"));
    el.scrollTop -= 200;
    el.dispatchEvent(new Event("scroll"));
    return { stuck: el.stuck, touching: el.touching, top: el.scrollTop, height: el.clientHeight, total: el.scrollHeight };
  });
  expect(state.stuck).toBe(false);
  expect(state.touching).toBe(true);
  const reads = await page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_view").length);
  await page.evaluate(next => window.__navigation.respond("agent_view", next), {
    ...view(active), entries: [...view(active).entries, { kind: "assistant", text: "New reply while reading older messages" }],
  });
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_view").length)).toBeGreaterThan(reads);
  await expect.poll(() => body.evaluate((el: HTMLElement & { stuck?: boolean }) => el.stuck)).toBe(false);
  expect(await details.evaluate(el => el === Reflect.get(window, "sessionHintsDisclosure") && el.hasAttribute("open"))).toBe(true);
  await expect(summary).toBeFocused();
  await body.dispatchEvent("touchend");
  await expect.poll(() => body.evaluate((el: HTMLElement & { touching?: boolean }) => el.touching)).toBe(false);
  await body.evaluate(el => { el.scrollTop = el.scrollHeight; el.dispatchEvent(new Event("scroll")); });
  await expect.poll(() => body.evaluate((el: HTMLElement & { stuck?: boolean }) => el.stuck)).toBe(true);
});

test("returning from Home opens the same conversation at its latest message", async ({ page }) => {
  const active = { ...session, status: "idle", external: false };
  await openDesktop(page, false, { agent_view: view(active) });
  const open = () => page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), active);
  const body = page.locator("#agbody");
  const gap = () => body.evaluate(el => el.scrollHeight - el.clientHeight - el.scrollTop);
  const jump = page.getByRole("button", { name: "Scroll to bottom" });

  await open();
  await expect(body).toContainText("Message 39");
  await expect.poll(gap).toBeLessThan(2);
  await page.keyboard.press("Home");
  await expect.poll(() => body.evaluate((el: HTMLElement & { top?: number }) => el.top)).toBe(0);
  await expect(jump).toBeVisible();

  await page.evaluate(() => window.__navigation.open("home"));
  await expect(body).toBeHidden();
  await open();
  await expect(body).toBeVisible();
  await expect(body.getByText("Message 39", { exact: false })).toBeInViewport();
  await expect.poll(gap).toBeLessThan(2);
  await expect(jump).toBeHidden();
});

test("reopening the desktop window returns the conversation to the latest message", async ({ page }) => {
  const active = { ...session, status: "idle", external: false };
  await openDesktop(page, false, { agent_view: view(active) });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), active);
  const body = page.locator("#agbody");
  const jump = page.getByRole("button", { name: "Scroll to bottom" });
  await expect(body).toContainText("Message 39");
  await expect(body.getByText("Message 39", { exact: false })).toBeInViewport();
  await page.keyboard.press("Home");
  await expect.poll(() => body.evaluate((el: HTMLElement & { top?: number }) => el.top)).toBe(0);
  await expect(jump).toBeVisible();

  await page.evaluate(() => {
    window.__navigation.emit("center-hidden", null);
    window.__navigation.emit("center-shown", null);
  });
  await expect(jump).toBeVisible();

  await page.evaluate(() => window.__navigation.emit("center-reopened", null));
  await expect(body.getByText("Message 39", { exact: false })).toBeInViewport();
  await expect.poll(() => body.evaluate(el => el.scrollHeight - el.clientHeight - el.scrollTop)).toBeLessThan(2);
  await expect(jump).toBeHidden();
});

for (const custom of [
  { id: "gpt-6.1-sol", label: "6.1 Sol", models: [{ id: "gpt-6.1-sol", name: "6.1 Sol", description: null }] },
  { id: "custom-model", label: "custom-model", models: [] },
]) {
  test(`a session without a reported model shows the custom default ${custom.id}`, async ({ page }) => {
    const active = { ...session, status: "working", external: false };
    await openDesktop(page, false, {
      agent_view: view(active),
      agent_models: { pick: "listed", current: custom.id, models: custom.models },
    });
    await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), active);
    await expect(page.locator("#agmodelb")).toContainText(custom.label);
    await expect(page.locator("#agmodelb")).not.toContainText("Model unknown");
    expect(await page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_steer"))).toEqual([]);
  });
}

test("a reported session model takes precedence over the configured default", async ({ page }) => {
  const active = { ...session, status: "working", external: false };
  await openDesktop(page, false, {
    agent_view: { ...view(active), model: "session-model" },
    agent_models: { pick: "listed", current: "custom-model", models: [{ id: "session-model", name: "Session model", description: null }] },
  });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), active);
  await expect(page.locator("#agmodelb")).toContainText("Session model");
});

test("conversation scrollbar preferences apply immediately and survive reload", async ({ page }) => {
  await openDesktop(page, false, { agent_view: view(), settings: { name: "build-box", version: "0.1", autostart: false, agent_notices: false } });
  const openSession = () => page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), session);
  const openAppearance = async () => {
    // The project's sessions stand in for the sidebar beside its session.
    await page.locator("#rail .cc-side-toggle").click();
    await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
    await page.getByRole("dialog", { name: "Settings" }).getByRole("button", { name: "Appearance", exact: true }).click();
  };
  await openSession();
  await expect(page.locator("#rail")).toBeVisible();
  const rail = page.locator(".agrail"), body = page.locator("#agbody");
  await expect(rail.locator(".th")).toBeVisible();
  await expect(rail.locator(".tk").first()).toBeVisible();
  await body.evaluate(el => { el.scrollTop = 200; el.dispatchEvent(new Event("scroll")); });
  const position = await body.evaluate(el => el.scrollTop);
  await openAppearance();
  const settings = page.getByRole("dialog", { name: "Settings" });
  const marks = settings.getByRole("switch", { name: "Prompt marks", exact: true });
  const scrollbar = settings.getByRole("switch", { name: "Conversation scrollbar", exact: true });
  await marks.click();
  await expect(marks).toHaveAttribute("aria-checked", "false");
  await expect(rail.locator(".tk")).toHaveCount(0);
  await expect(rail.locator(".th")).toBeVisible();
  await scrollbar.click();
  await expect(rail).toHaveCount(0);
  expect(await body.evaluate(el => el.scrollTop)).toBe(position);
  await scrollbar.click();
  await expect(rail.locator(".th")).toBeVisible();
  await expect(rail.locator(".tk")).toHaveCount(0);
  await scrollbar.click();
  await page.reload();
  await expect.poll(() => page.evaluate(() => window.__navigation.ready())).toBe(true);
  await openSession();
  await expect(body).toContainText("Message 39");
  await expect(rail).toHaveCount(0);
  await body.evaluate(el => { el.scrollTop = 100; el.dispatchEvent(new Event("scroll")); });
  expect(await body.evaluate(el => el.scrollTop)).toBe(100);
  await openAppearance();
  await expect(scrollbar).toHaveAttribute("aria-checked", "false");
  await expect(marks).toHaveAttribute("aria-checked", "false");
  await scrollbar.click();
  await marks.click();
  await expect(rail.locator(".th")).toBeVisible();
  await expect(rail.locator(".tk").first()).toBeVisible();
});

test("sending an image keeps a conversation that was at the bottom following its end", async ({ page }) => {
  const active = { ...session, status: "idle", external: false };
  await openDesktop(page, false, { agent_view: view(active), agent_tree_upload: "/cache/uploads/shot.png", agent_steer: ["uploaded-picture"] });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), active);
  const body = page.locator("#agbody");
  await expect(body).toContainText("Message 39");
  const png = await page.evaluate(() => { const canvas = document.createElement("canvas"); canvas.width = 400; canvas.height = 300; return canvas.toDataURL("image/png").split(",")[1]; });
  await page.locator('#agfield input[type="file"]').setInputFiles({ name: "shot.png", mimeType: "image/png", buffer: Buffer.from(png, "base64") });
  await expect(page.locator("#agfield .attachment-count")).toHaveText("1 file");
  await page.locator("#agmsg").fill("Describe the attached picture");
  await body.evaluate((el: HTMLElement & { stuck?: boolean }) => { el.stuck = true; el.scrollTop = el.scrollHeight; });
  await page.evaluate(() => window.__navigation.holdNext("agent_steer"));
  await page.locator("#agmsg").press("Enter");
  await expect(body).toContainText("Describe the attached picture");
  await expect.poll(() => body.evaluate(el => el.scrollHeight - el.scrollTop - el.clientHeight)).toBeLessThan(3);
  await expect.poll(() => body.evaluate((el: HTMLElement & { stuck?: boolean }) => el.stuck)).toBe(true);
});


test("two files with one name sent from this window open as what the machine holds at each path", async ({ page }) => {
  const active = { ...session, status: "idle", external: false };
  const paths = ["/cache/uploads/one/recording.webm", "/cache/uploads/two/recording.webm"];
  await openDesktop(page, false, { agent_view: view(active), agent_steer: [], agent_file: { size: 3, mime: "video/webm", data: btoa("abc") } });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), active);
  await expect(page.locator("#agbody")).toContainText("Message 39");
  for (const [i, text] of ["First video", "Second video"].entries()) {
    await page.locator('#agfield input[type="file"]').setInputFiles({ name: "recording.webm", mimeType: "video/webm", buffer: Buffer.from(`bytes ${i}`) });
    await expect(page.locator("#agfield .attachment-count")).toHaveText("1 file");
    await page.locator("#agmsg").fill(text);
    await page.locator("#agmsg").press("Enter");
  }
  const entries = paths.map((path, i) => ({ kind: "user", text: `${i ? "Second" : "First"} video\n\n@"${path}"`, html: `<p>${i ? "Second" : "First"} video</p><p>@&quot;${path}&quot;</p>` }));
  await page.evaluate(({ active, next, uploaded }) => {
    window.__navigation.respond("agent_view", next);
    window.__navigation.emit("outbox-changed", { machine_id: active.machine_id, agent: active.agent, session: active.id, revision: 1, rows: [], sent: [], resumed: null, uploaded });
  }, { active, next: { ...view(active), entries: [...view(active).entries, ...entries] }, uploaded: paths.map(path => ({ name: "recording.webm", path })) });
  const buttons = page.locator("#agbody").getByRole("button", { name: "recording.webm", exact: true });
  await expect(buttons).toHaveCount(2);
  await buttons.nth(0).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_file").map(call => call.args.path))).toEqual([paths[0]]);
});

test("a pending attachment send finishes in its original conversation after navigation", async ({ page }) => {
  const active = { ...session, status: "idle", external: false };
  const other = { ...active, id: "other-session", title: "Another conversation" };
  await openDesktop(page, false, { agent_view: view(active), agent_tree_upload: "/cache/uploads/notes.txt", agent_steer: [] });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), active);
  await expect(page.locator("#agbody")).toContainText("Message 39");
  await page.locator('#agfield input[type="file"]').setInputFiles({ name: "notes.txt", mimeType: "text/plain", buffer: Buffer.from("Notes for the original conversation") });
  await expect(page.locator("#agfield .attachment-count")).toHaveText("1 file");
  await page.locator("#agmsg").fill("Use these notes");
  await page.locator("#agmsg").press("Enter");
  await expect(page.locator("#agmsg")).toHaveValue("");
  await page.evaluate(({ other, next }) => { window.__navigation.respond("agent_view", next); window.__navigation.open({ agent: { ...other, kind: "agent" } }); }, { other, next: view(other) });
  await page.locator("#agmsg").fill("Draft for the other conversation");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_steer").length)).toBe(1);
  const sends = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_steer"));
  expect(sends[0].args).toMatchObject({ session: active.id, steer: { steer: { text: expect.stringContaining("Use these notes") } } });
  await expect(page.locator("#agmsg")).toHaveValue("Draft for the other conversation");
  await expect(page.locator("#agfield .attachment-count")).toHaveCount(0);
});

test("fast mode alone is applied before continuing an ended session", async ({ page }) => {
  const resumed = { ...session, id: "resumed-speed", external: false, status: "idle" };
  await openDesktop(page, false, {
    agent_view: view(), agent_resume: { session: resumed, why: "" }, agent_steer: [],
    agent_models: { pick: "listed", current: "model-a", models: [{ id: "model-a", name: "Example model", description: null, fast: true, efforts: ["low", "ultra"] }] },
  });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), session);
  await page.locator("#agmodelb").click();
  const picker = page.getByRole("dialog", { name: "Harness, model and thinking" });
  await picker.getByRole("switch", { name: "Fast mode", exact: true }).click();
  await page.locator("#agmsg").click();
  await page.locator("#agmsg").fill("Continue at fast speed");
  await page.locator("#agmsg").press("Enter");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_steer").length)).toBe(1);
  const calls = await page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_resume" || c.command === "agent_steer"));
  expect(calls.map(c => c.command)).toEqual(["agent_steer"]);
  expect(calls[0].args).toMatchObject({ steer: { steer: { text: "Continue at fast speed" } }, resume: { pick: { fast: true } } });
});

test("the model shortcut returns focus to the conversation input after using the picker", async ({ page }) => {
  const active = { ...session, status: "working", external: false };
  await openDesktop(page, false, { agent_view: view(active) });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), active);
  const input = page.locator("#agmsg");
  await expect(page.locator("#agmodelb")).toBeEnabled();
  await input.focus();
  for (let repeat = 0; repeat < 2; repeat++) {
    await page.keyboard.press("Meta+Shift+m");
    await expect(page.locator(".agc-search")).toBeFocused();
    await page.keyboard.press("Meta+Shift+m");
    await expect(input).toBeFocused();
  }
});
