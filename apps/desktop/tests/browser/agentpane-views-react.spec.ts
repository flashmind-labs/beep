import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const listing = (names: string[]) => ({ folders: [{ path: "", entries: names.map(name => ({ name, folder: false, size: 1, ignored: false, change: null })), more: 0 }], truncated: false });
const agent = { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false };

/// One triangle that slides along x over a second.
function animatedTriangle() {
  const floats = new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 1, 0, 0]);
  const uri = `data:application/octet-stream;base64,${Buffer.from(floats.buffer).toString("base64")}`;
  const gltf = {
    asset: { version: "2.0" },
    scene: 0, scenes: [{ nodes: [0] }], nodes: [{ mesh: 0 }],
    meshes: [{ primitives: [{ attributes: { POSITION: 0 } }] }],
    animations: [{ name: "Slide", channels: [{ sampler: 0, target: { node: 0, path: "translation" } }], samplers: [{ input: 1, output: 2 }] }],
    buffers: [{ byteLength: 68, uri }],
    bufferViews: [{ buffer: 0, byteOffset: 0, byteLength: 36 }, { buffer: 0, byteOffset: 36, byteLength: 8 }, { buffer: 0, byteOffset: 44, byteLength: 24 }],
    accessors: [
      { bufferView: 0, componentType: 5126, count: 3, type: "VEC3", min: [0, 0, 0], max: [1, 1, 0] },
      { bufferView: 1, componentType: 5126, count: 2, type: "SCALAR", min: [0], max: [1] },
      { bufferView: 2, componentType: 5126, count: 2, type: "VEC3" },
    ],
  };
  const text = JSON.stringify(gltf);
  return { size: text.length, mime: "model/gltf+json", data: Buffer.from(text).toString("base64") };
}

test("a model tab keeps its canvas and focus, and draws again after reopening the session", async ({ page }) => {
  await openDesktop(page, true, { agent_tree: listing(["scene.gltf"]), agent_file: animatedTriangle() });
  await page.evaluate(value => window.__navigation.open({ agent: value }), agent);
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  await page.locator('[data-fpath="scene.gltf"]').click();
  const stage = page.locator(".agpstage.model"), canvas = stage.locator("canvas");
  await expect(canvas).toHaveCount(1);
  await expect(stage.locator(".agpmsg")).toHaveCount(0);
  await expect(page.locator(".agpacts.anim")).toBeVisible();
  await expect(page.getByRole("button", { name: /^Clip/ })).toHaveCount(0);
  await canvas.evaluate(element => { element.dataset.identity = "kept"; });
  const wire = page.getByRole("button", { name: "Wireframe" });
  await wire.click();
  await expect(wire).toHaveClass(/on/);
  await expect(wire).toBeFocused();
  await expect(page.locator(".agpacts.model-actions .meta")).toContainText("glTF");
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  await page.getByRole("tab", { name: "scene.gltf" }).click();
  await expect(canvas).toHaveAttribute("data-identity", "kept");
  await expect(wire).toHaveClass(/on/);

  await page.evaluate(() => window.__navigation.open("home"));
  await page.evaluate(value => window.__navigation.open({ agent: value }), agent);
  await page.getByRole("tab", { name: "scene.gltf" }).click();
  await expect(page.locator(".agpstage.model")).toHaveCount(1);
  await expect(canvas).toHaveCount(1);
  await expect(canvas).not.toHaveAttribute("data-identity", "kept");
  await expect(page.getByRole("button", { name: "Reset view" })).toBeEnabled();
  await page.getByRole("button", { name: "Wireframe" }).click();
  await expect(page.getByRole("button", { name: "Wireframe" })).toHaveClass(/on/);
  await page.getByRole("button", { name: "Loop" }).click();
  await expect(page.getByRole("button", { name: "Loop" })).toHaveAttribute("aria-pressed", "false");
});
