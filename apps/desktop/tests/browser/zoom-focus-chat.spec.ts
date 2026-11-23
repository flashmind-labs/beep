import { readFileSync } from "node:fs";
import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

// The script the app puts at the start of every HTML page it serves.
const paneScript = readFileSync(new URL("../../src-tauri/src/agents/preview-script.html", import.meta.url), "utf8");

async function openPreview(page: Page, html: string) {
  await page.addInitScript(() => localStorage.setItem("beep.projectView.build-box\nnotice-one", JSON.stringify({
    on: { right: "text:page.html", bottom: "text:page.html" }, tabs: [{ root: "/srv/project", rel: "page.html" }],
  })));
  const src = "https://preview.test/page.html";
  await page.route("https://preview.test/**", route => route.fulfill({
    contentType: "text/html",
    body: paneScript + (route.request().url() === src ? html : "<p>Next page</p>"),
  }));
  await openDesktop(page, true, { agent_artifact: { kind: "html", size: html.length, src } });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex",
    title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  const tab = page.getByRole("tab", { name: /^page.html Close/ });
  await expect(tab).toHaveAttribute("aria-selected", "true");
  await page.getByRole("button", { name: "Preview", exact: true }).click();
  return tab;
}

test("a focused HTML preview hands ⇧esc, ⌘1 and ⌘W to the pane", { tag: "@windows" }, async ({ page }, info) => {
  const command = info.project.name === "Windows" ? "Control" : "Meta";
  const tab = await openPreview(page, "<p>Preview</p>");
  const preview = page.frameLocator(".agartframe").getByText("Preview");

  await preview.click();
  await page.keyboard.press("Shift+Escape");
  await expect(page.locator("#agshell")).toHaveClass(/zoomed/);
  await preview.click();
  await page.keyboard.press(`${command}+1`);
  await expect(page.locator("#agshell")).not.toHaveClass(/zoomed/);
  await expect(page.locator("#agmsg")).toBeFocused();

  await preview.click();
  await page.keyboard.press(`${command}+w`);
  await expect(tab).toHaveCount(0);
});

test("external preview links open in the system browser and anchors stay in the frame", async ({ page }) => {
  const url = "https://example.test/guide?q=1#details";
  await openPreview(page, `<a href="${url}" target="_blank"><span>Guide</span></a>
    <a href="http://example.test/">Home</a><a href="#details">Details</a><p id="details">Section</p><a href="next.html">Next page</a>`);
  const frame = page.frameLocator(".agartframe");
  const opened = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "open_url").map(call => call.args.url));

  await frame.getByText("Guide").click({ button: "middle" });
  await expect.poll(opened).toEqual([url]);
  await frame.getByText("Guide").click();
  await expect.poll(opened).toEqual([url, url]);
  await frame.getByRole("link", { name: "Home" }).press("Enter");
  await expect.poll(opened).toEqual([url, url, "http://example.test/"]);

  const details = frame.getByRole("link", { name: "Details" });
  await details.click();
  await expect.poll(() => details.evaluate(() => location.hash)).toBe("#details");
  await frame.getByRole("link", { name: "Next page" }).click();
  await expect(frame.getByText("Next page", { exact: true })).toHaveJSProperty("tagName", "P");
  expect(await opened()).toEqual([url, url, "http://example.test/"]);
});
