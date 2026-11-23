import { readFileSync } from "node:fs";
import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

// The script the app puts at the start of every HTML page it serves.
const paneScript = readFileSync(new URL("../../src-tauri/src/agents/preview-script.html", import.meta.url), "utf8");

async function openPreview(page: Page, html: string, replies: Record<string, unknown> = {}) {
  await page.addInitScript(() => localStorage.setItem("beep.projectView.build-box\nnotice-one", JSON.stringify({
    on: { right: "text:page.html", bottom: "text:page.html" }, tabs: [{ root: "/srv/project", rel: "page.html", preview: true }],
  })));
  const src = "https://preview.test/page.html";
  await page.route("https://preview.test/**", route => route.fulfill({
    contentType: "text/html",
    body: paneScript + (route.request().url() === src ? html : "<p>Next page</p>"),
  }));
  await openDesktop(page, true, { agent_artifact: { kind: "html", size: html.length, src }, ...replies });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex",
    title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  const tab = page.getByRole("tab", { name: /^page.html Close/ });
  await expect(tab).toHaveAttribute("aria-selected", "true");
  await expect(page.locator(".agartframe")).toBeVisible();
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

test("a generated SVG saves locally and opens as an image while the source stays open", async ({ page }) => {
  const path = "/home/user/Downloads/graph.svg";
  const svg = '<svg xmlns="http://www.w3.org/2000/svg" width="120" height="80"><rect width="120" height="80"/><script>parent.postMessage("svg ran", "*")</script></svg>';
  await page.addInitScript(() => {
    window.addEventListener("message", event => {
      if (event.data === "svg ran") document.body.dataset.svgRan = "yes";
    });
  });
  const source = await openPreview(page, `<button>Export SVG</button><script>
    function exportSvg() {
      const link = document.createElement("a");
      link.href = URL.createObjectURL(new Blob([${JSON.stringify(svg).replace("</script>", "<\\/script>")}], { type: "image/svg+xml" }));
      link.download = "graph.svg";
      link.click();
      URL.revokeObjectURL(link.href);
    }
    exportSvg();
    document.querySelector("button").addEventListener("click", event => {
      event.currentTarget.textContent = "Export again";
      exportSvg();
    });
  </script>`, { agent_preview_export: path });

  const frame = page.frameLocator(".agartframe");
  const calls = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_preview_export").map(call => call.args));
  await expect(frame.getByRole("button", { name: "Export SVG" })).toBeVisible();
  expect(await calls()).toEqual([]);
  await frame.getByRole("button", { name: "Export SVG" }).click();

  await expect(page.getByRole("tab", { name: /^graph.svg Close/ })).toHaveAttribute("aria-selected", "true");
  await expect.poll(calls).toEqual([{ name: "graph.svg", kind: "svg", data: Buffer.from(svg).toString("base64") }]);
  const image = page.getByRole("img", { name: "graph.svg" });
  await expect.poll(() => image.evaluate(el => el instanceof HTMLImageElement && el.naturalWidth)).toBe(120);
  await expect(page.locator("body")).not.toHaveAttribute("data-svg-ran", "yes");
  await expect(page.getByText("Saved to Downloads")).toBeVisible();
  await page.getByRole("button", { name: "Show in folder" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().find(call => call.command === "reveal_file")?.args)).toEqual({ path });

  await source.click();
  await expect(frame.getByRole("button", { name: "Export again" })).toBeVisible();
});

test("a PDF download link saves its bytes and opens a PDF tab", async ({ page }) => {
  const path = "/home/user/Downloads/graph.pdf";
  const data = pdfFixture();
  await openPreview(page, `<a href="data:application/pdf;base64,${data}" download="graph.pdf">Export PDF</a>`, { agent_preview_export: path });
  await page.frameLocator(".agartframe").getByRole("link", { name: "Export PDF" }).click();
  await expect(page.getByRole("tab", { name: /^graph.pdf Close/ })).toHaveAttribute("aria-selected", "true");
  const first = page.getByLabel("graph.pdf, page 1");
  await expect(first).toBeVisible();
  expect(await first.evaluate(canvas => canvas instanceof HTMLCanvasElement &&
    Array.from(canvas.getContext("2d")?.getImageData(canvas.width / 2, canvas.height / 2, 1, 1).data ?? []))).toEqual([255, 0, 0, 255]);
  await page.getByRole("button", { name: "Next", exact: true }).click();
  const second = page.getByLabel("graph.pdf, page 2");
  await expect(second).toBeVisible();
  expect(await second.evaluate(canvas => canvas instanceof HTMLCanvasElement &&
    Array.from(canvas.getContext("2d")?.getImageData(canvas.width / 2, canvas.height / 2, 1, 1).data ?? []))).toEqual([0, 0, 255, 255]);
  expect(await page.evaluate(() => window.__navigation.calls().find(call => call.command === "agent_preview_export")?.args)).toEqual({ name: "graph.pdf", kind: "pdf", data });
});

function pdfFixture() {
  const page = (content: number) => `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents ${content} 0 R >>`;
  const fill = (color: string) => {
    const stream = `${color} rg\n0 0 100 100 re f\n`;
    return `<< /Length ${stream.length} >>\nstream\n${stream}endstream`;
  };
  const objects = [
    "<< /Type /Catalog /Pages 2 0 R >>",
    "<< /Type /Pages /Count 2 /Kids [3 0 R 4 0 R] >>",
    page(5), page(6), fill("1 0 0"), fill("0 0 1"),
  ];
  let pdf = "%PDF-1.4\n";
  const offsets = objects.map((object, index) => {
    const offset = pdf.length;
    pdf += `${index + 1} 0 obj\n${object}\nendobj\n`;
    return String(offset).padStart(10, "0") + " 00000 n \n";
  });
  return Buffer.from(pdf + `xref\n0 7\n0000000000 65535 f \n${offsets.join("")}trailer\n<< /Size 7 /Root 1 0 R >>\nstartxref\n${pdf.length}\n%%EOF\n`).toString("base64");
}
