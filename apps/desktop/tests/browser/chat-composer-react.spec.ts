import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("chat updates keep the composer, draft, caret and scroll", async ({ page }) => {
  const history = [{ role: "user", text: "Explain", at: 1, images: [], model: "", thinking: "" }, { role: "assistant", html: "<p>Answer</p>", tools: [], replies: [], model: "" }];
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: history, session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const ask = page.locator("#ask"), body = page.locator("#body");
  await expect(page.locator(".dcoderen.agent")).toContainText("Answer");
  await ask.fill("draft text");
  await ask.evaluate(el => { if (el instanceof HTMLTextAreaElement) { el.setSelectionRange(2, 5); Reflect.set(window, "chatComposer", el); } });
  await body.evaluate(el => { el.style.height = "40px"; el.style.flex = "0 0 40px"; el.style.overflow = "auto"; el.scrollTop = el.scrollHeight; el.dispatchEvent(new Event("scroll")); el.scrollTop = 10; el.dispatchEvent(new Event("scroll")); });
  await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => resolve())));
  const top = await body.evaluate(el => el.scrollTop);
  await page.evaluate(() => window.__navigation.emit("sessions-changed", "chat-example"));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "open_session").length)).toBeGreaterThan(1);
  expect(await ask.evaluate(el => el === Reflect.get(window, "chatComposer") && document.activeElement === el && el instanceof HTMLTextAreaElement && el.value === "draft text" && el.selectionStart === 2 && el.selectionEnd === 5)).toBe(true);
  await expect(body).toHaveJSProperty("scrollTop", top);
  await expect(page.getByRole("separator", { name: "Resize the prompt" })).toBeVisible();
});

test("chat shell keeps the shared bar root through navigation", async ({ page }) => {
  const history = [{ role: "user", text: "Explain", at: 1, images: [], model: "", thinking: "" }, { role: "assistant", html: "<p>Answer</p>", tools: [], replies: [], model: "" }];
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: history, session_agents: [], session_files: [], session_sources: [], list_questions: [], list_researches: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  await expect(page.locator("#ask")).toBeVisible();
  await page.locator("#bar > [data-bar-page]").evaluate(el => Reflect.set(window, "chatPageHost", el));
  await page.locator("#ask").fill("A draft");
  await expect(page.locator("#bar .dhints")).toContainText("ask");
  await page.evaluate(() => window.__navigation.open("research"));
  await expect(page.getByPlaceholder("What should the team research?")).toBeVisible();
  expect(await page.locator("#bar > [data-bar-page]").evaluate(el => el === Reflect.get(window, "chatPageHost"))).toBe(true);
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  await expect(page.locator("#ask")).toBeVisible();
  expect(await page.locator("#bar > [data-bar-page]").evaluate(el => el === Reflect.get(window, "chatPageHost"))).toBe(true);
  await expect(page.locator("#body")).toContainText("Answer");
});

test("bar chat keeps hints and measures its shared shell", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: [], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.goto("https://app.test/index.html");
  const ask = page.locator("#ask");
  await expect(ask).toBeVisible();
  await ask.fill("A question");
  await expect(page.locator("#bar .dhints")).toContainText("ask");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "resize_bar").map(call => Number(call.args.height)).at(-1) ?? 0)).toBeGreaterThan(50);
});

test("leaving live chat removes its dock when switching pages", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: true, session: null }, open_session: [], session_agents: [], session_files: [], session_sources: [], list_questions: [], list_researches: [] });
  await page.goto("https://app.test/index.html");
  await expect(page.locator("#ask")).toBeVisible();
  await page.evaluate(() => window.__navigation.emit("live", { kind: "view", phase: "listening", words: "", line: null, prev: null, step: null, speaking: false, pause_sends: false, session: null, title: null, hold_only: false, finishing: false, sheet: false }));
  await expect(page.locator("#ldock")).toBeVisible();
  await page.evaluate(() => window.__navigation.emit("bar-shown", "research"));
  await expect(page.getByPlaceholder("What should the team research?")).toBeVisible();
  await expect(page.locator("#ldock")).toHaveCount(0);
});

test("chat queue updates keep the prompt and running controls mounted", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: [], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => { const invoke = window.__TAURI__.core.invoke; window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => command === "send_prompt" ? new Promise<T>(() => {}) : invoke<T>(command, args); });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const ask = page.locator("#ask");
  await ask.fill("First request");
  await ask.press("Enter");
  await expect(page.locator("#stopRun")).toBeVisible();
  await ask.fill("First queued");
  await ask.press("Enter");
  const first = page.locator("#queue .it").filter({ hasText: "First queued" });
  await first.evaluate(el => Reflect.set(window, "queuedFirst", el));
  await ask.fill("Second queued");
  await ask.press("Enter");
  expect(await first.evaluate(el => el === Reflect.get(window, "queuedFirst"))).toBe(true);
  await expect(page.locator("#stopRun")).toBeVisible();
  await page.locator("#queue .it").filter({ hasText: "Second queued" }).getByRole("button", { name: "Edit" }).click();
  await expect(ask).toHaveValue("Second queued");
  await expect(ask).toBeFocused();
  await expect(first).toBeVisible();
});

test("attachment updates keep the chat draft and focus", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: [], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const ask = page.locator("#ask");
  await ask.fill("A draft");
  await ask.evaluate(el => { if (el instanceof HTMLTextAreaElement) { el.setSelectionRange(1, 3); Reflect.set(window, "attachmentDraft", el); } const data = new DataTransfer(); data.items.add(new File(["sample"], "example.txt", { type: "text/plain" })); el.dispatchEvent(new ClipboardEvent("paste", { bubbles: true, clipboardData: data })); });
  await expect(page.locator("#thumbs")).toContainText("example.txt");
  expect(await ask.evaluate(el => el === Reflect.get(window, "attachmentDraft") && document.activeElement === el && el instanceof HTMLTextAreaElement && el.selectionStart === 1 && el.selectionEnd === 3)).toBe(true);
  await page.getByRole("button", { name: "Remove example.txt" }).click();
  await expect(page.locator("#thumbs")).toHaveCount(0);
  await expect(ask).toHaveValue("A draft");
  await expect(ask).toBeFocused();
});

test("running chat retains its React-owned body across bar navigation", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: "chat-example" }, open_session: [], session_agents: [], session_files: [], session_sources: [], list_questions: [], list_researches: [] });
  await page.goto("https://app.test/index.html");
  await page.evaluate(() => { const invoke = window.__TAURI__.core.invoke; window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => command === "send_prompt" ? new Promise<T>(() => {}) : invoke<T>(command, args); });
  await page.evaluate(() => window.__navigation.emit("bar-shown", "open"));
  await page.locator("#ask").fill("Running request");
  await page.locator("#ask").press("Enter");
  await expect(page.locator("#stopRun")).toBeVisible();
  await page.locator("#body").evaluate(el => Reflect.set(window, "runningChatBody", el));
  await page.evaluate(() => window.__navigation.emit("bar-shown", "research"));
  await expect(page.getByPlaceholder("What should the team research?")).toBeVisible();
  await page.evaluate(() => window.__navigation.emit("bar-shown", "open"));
  await expect(page.locator("#body")).toContainText("Running request");
  expect(await page.locator("#body").evaluate(el => el === Reflect.get(window, "runningChatBody"))).toBe(true);
});

test("bar sizing observes turn boxes inside React hosts", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: [], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.addInitScript(() => {
    const Native = window.ResizeObserver, observed = new WeakSet<Element>();
    Reflect.set(window, "observedChatBoxes", observed);
    window.ResizeObserver = class extends Native { override observe(target: Element, options?: ResizeObserverOptions) { observed.add(target); super.observe(target, options); } };
  });
  await page.goto("https://app.test/index.html");
  await page.evaluate(() => { const invoke = window.__TAURI__.core.invoke; window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => command === "send_prompt" ? new Promise<T>(() => {}) : invoke<T>(command, args); });
  await page.locator("#ask").fill("Observe this turn");
  await page.locator("#ask").press("Enter");
  await expect(page.locator("#body .turn")).toBeVisible();
  await expect.poll(() => page.locator("#body .turn").evaluate(el => Reflect.get(window, "observedChatBoxes").has(el))).toBe(true);
});

test("composer overlay keeps paste pills and code outline through detach and reattach", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: [], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const ask = page.locator("#ask");
  await ask.fill("Explain\n\n\n");
  await ask.evaluate(el => {
    if (!(el instanceof HTMLTextAreaElement)) return;
    el.setSelectionRange(el.value.length, el.value.length);
    const data = new DataTransfer(); data.setData("text/plain", Array.from({ length: 12 }, (_, index) => `line ${index}`).join("\n"));
    el.dispatchEvent(new ClipboardEvent("paste", { bubbles: true, cancelable: true, clipboardData: data }));
  });
  const layer = page.locator(".cx-layer");
  await expect(layer.locator(".cx-pill")).toHaveCount(1);
  await expect(layer.locator(".cx-code")).toHaveCount(1);
  await ask.evaluate(el => {
    const box = el.parentElement?.querySelector(".cx-pill")?.getBoundingClientRect();
    if (box) el.dispatchEvent(new MouseEvent("click", { bubbles: true, clientX: box.left + 1, clientY: box.top + 1 }));
  });
  await expect(page.locator(".cx-pop")).toContainText("Pasted text");
  await ask.press("Escape");
  await expect(page.locator(".cx-pop")).toHaveCount(0);
  await ask.evaluate(el => { Reflect.set(window, "composeField", el); Reflect.set(window, "composeParent", el.parentElement); el.parentElement?.remove(); });
  await expect.poll(() => page.evaluate(() => (Reflect.get(window, "composeField") as HTMLElement).isConnected)).toBe(false);
  await page.evaluate(() => document.querySelector("#bar")?.append(Reflect.get(window, "composeParent")));
  await expect(layer.locator(".cx-pill")).toHaveCount(1);
  await expect(layer.locator(".cx-code")).toHaveCount(1);
  expect(await ask.evaluate(el => el === Reflect.get(window, "composeField"))).toBe(true);
  await ask.evaluate(el => {
    const box = el.parentElement?.querySelector(".cx-pill")?.getBoundingClientRect();
    if (box) el.dispatchEvent(new MouseEvent("click", { bubbles: true, clientX: box.left + 1, clientY: box.top + 1 }));
  });
  await page.locator(".cx-pop").getByRole("button", { name: "Expand" }).click();
  await expect(ask).toHaveValue(/line 0.*line 11/s);
  await expect(ask).toBeFocused();
  await expect(page.locator(".cx-pop")).toHaveCount(0);
});

test("file drag overlay stays mounted until the pointer leaves", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: [], session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  await expect(page.locator("#ask")).toBeVisible();
  await page.evaluate(() => {
    const data = new DataTransfer(); data.items.add(new File(["data"], "example.txt"));
    const bar = document.querySelector("#bar");
    bar?.dispatchEvent(new DragEvent("dragover", { bubbles: true, cancelable: true, dataTransfer: data }));
    Reflect.set(window, "dropOverlay", document.querySelector("#ddrop"));
    bar?.dispatchEvent(new DragEvent("dragover", { bubbles: true, cancelable: true, dataTransfer: data }));
  });
  const overlay = page.locator("#ddrop");
  await expect(overlay).toContainText("Drop images or files to ask about them");
  expect(await overlay.evaluate(el => el === Reflect.get(window, "dropOverlay"))).toBe(true);
  await page.evaluate(() => document.querySelector("#bar")?.dispatchEvent(new DragEvent("dragleave", { bubbles: true })));
  await expect(overlay).toHaveCount(0);
});

test("chat composer shows the Agent button once a machine offers agents", async ({ page }) => {
  const hosts = [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: "/srv/project", home: "/path/to/work", capabilities: [] }];
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: [], session_agents: [], session_files: [], session_sources: [], list_questions: [], agent_hosts: hosts });
  await page.evaluate(() => window.__navigation.open({ chat: { session: "chat-example" } }));
  const agent = page.locator(".dinput.dgrow #agentGo");
  await expect(agent).toBeVisible();
  await expect(agent).toHaveAccessibleName("Agent");
  await page.locator("#ask").fill("Fix the build");
  await page.evaluate(() => window.__navigation.emit("sessions-changed", "chat-example"));
  await expect(page.locator("#agentGo")).toHaveCount(1);
  await agent.click();
  await expect(page.locator("#agentGo")).toHaveCount(0);
  await expect(page.locator("#ask")).toHaveCount(0);
});
