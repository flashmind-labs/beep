// The media pane's 3D viewer: a glTF, GLB, OBJ or STL model the agent made,
// drawn with three.js, framed to fit, lit simply, turned with the pointer.

import {
  AmbientLight, Box3, Color, DirectionalLight, GridHelper, Group, HemisphereLight, Mesh, MeshStandardMaterial,
  type Object3D, PerspectiveCamera, Scene, SRGBColorSpace, Texture, TextureLoader, Vector3, WebGLRenderer,
} from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { OBJLoader } from "three/examples/jsm/loaders/OBJLoader.js";
import { STLLoader } from "three/examples/jsm/loaders/STLLoader.js";

/// What the viewer says of a model under it.
export interface ModelInfo {
  format: string;
  triangles: number;
  meshes: number;
}

export interface ModelView {
  info: ModelInfo;
  /// Frames the whole model again.
  reset(): void;
  wireframe(on: boolean): void;
  /// Draws again after the box changed size.
  resize(): void;
  dispose(): void;
}

/// Reads a file a `.gltf` names next to it (a buffer or texture), by its
/// relative URI; the bytes, or null when it cannot be read.
export type Sibling = (uri: string) => Promise<Uint8Array | null>;

const GRAY = new MeshStandardMaterial({ color: 0xb8c4bd, roughness: 0.6, metalness: 0.05 });

/// Parses `bytes` of a model file of extension `ext` into a scene node.
async function parse(bytes: ArrayBuffer, ext: string, sibling: Sibling): Promise<{ root: Object3D; format: string }> {
  if (ext === "stl") {
    const geometry = new STLLoader().parse(bytes);
    geometry.computeVertexNormals();
    return { root: new Mesh(geometry, GRAY.clone()), format: "STL" };
  }
  if (ext === "obj") {
    const root = new OBJLoader().parse(new TextDecoder().decode(bytes));
    // An OBJ's materials live in an .mtl the viewer does not read.
    root.traverse(o => { if (o instanceof Mesh) o.material = GRAY.clone(); });
    return { root, format: "OBJ" };
  }
  const glb = ext === "gltf" ? await packGltf(bytes, sibling) : bytes;
  const loader = new GLTFLoader();
  // Textures load as <img>, which the window's content policy allows for
  // blob: URLs, rather than with fetch, which it does not.
  loader.register(parser => {
    parser.textureLoader = new TextureLoader(parser.options.manager);
    return { name: "beep_textures" };
  });
  const gltf = await loader.parseAsync(glb, "");
  return { root: gltf.scene, format: ext === "gltf" ? "glTF" : "GLB" };
}

const b64 = (s: string) => Uint8Array.from(atob(s), c => c.charCodeAt(0));
const pad4 = (n: number) => (n + 3) & ~3;

/// A `.gltf` and the files it names as one GLB, so nothing is fetched by
/// URL: its buffers (beside it or in data URIs) joined into the binary
/// chunk, and its pictures beside it as blob URLs.
async function packGltf(bytes: ArrayBuffer, sibling: Sibling): Promise<ArrayBuffer> {
  const json = JSON.parse(new TextDecoder().decode(bytes));
  const buffers: { uri?: string; byteLength: number }[] = json.buffers ?? [];
  const parts: Uint8Array[] = [];
  const base: number[] = [];
  let length = 0;
  for (const b of buffers) {
    const data = !b.uri ? new Uint8Array(b.byteLength)
      : b.uri.startsWith("data:") ? b64(b.uri.slice(b.uri.indexOf(",") + 1))
      : await sibling(b.uri);
    if (!data) throw new Error(`${b.uri} is not there`);
    base.push(length);
    parts.push(data);
    length = pad4(length + data.length);
  }
  for (const v of json.bufferViews ?? []) {
    v.byteOffset = (v.byteOffset ?? 0) + base[v.buffer];
    v.buffer = 0;
  }
  json.buffers = length ? [{ byteLength: length }] : [];
  for (const img of json.images ?? []) {
    if (!img.uri || img.uri.startsWith("data:")) continue;
    const data = await sibling(img.uri);
    if (data) img.uri = URL.createObjectURL(new Blob([data as BlobPart], { type: img.mimeType || "image/png" }));
  }
  const text = new TextEncoder().encode(JSON.stringify(json));
  const jsonLen = pad4(text.length), binLen = pad4(length);
  const total = 12 + 8 + jsonLen + (length ? 8 + binLen : 0);
  const out = new Uint8Array(total), view = new DataView(out.buffer);
  view.setUint32(0, 0x46546c67, true);
  view.setUint32(4, 2, true);
  view.setUint32(8, total, true);
  view.setUint32(12, jsonLen, true);
  view.setUint32(16, 0x4e4f534a, true);
  out.set(text, 20);
  out.fill(0x20, 20 + text.length, 20 + jsonLen);
  if (length) {
    const at = 20 + jsonLen;
    view.setUint32(at, binLen, true);
    view.setUint32(at + 4, 0x004e4942, true);
    parts.forEach((p, i) => out.set(p, at + 8 + base[i]));
  }
  return out.buffer;
}

/// Draws the model in `bytes` into `box`, filling it.
export async function mountModel(box: HTMLElement, bytes: ArrayBuffer, ext: string, sibling: Sibling): Promise<ModelView> {
  const { root, format } = await parse(bytes, ext, sibling);
  const renderer = new WebGLRenderer({ antialias: true, alpha: true });
  renderer.setPixelRatio(Math.min(2, devicePixelRatio));
  renderer.outputColorSpace = SRGBColorSpace;
  box.append(renderer.domElement);

  const scene = new Scene();
  scene.add(new HemisphereLight(0xe4eee8, 0x1a231e, 1.2), new AmbientLight(0xffffff, 0.25));
  const key = new DirectionalLight(0xffffff, 1.6), fill = new DirectionalLight(0x9dc3f0, 0.5);
  scene.add(key, fill);
  const holder = new Group();
  holder.add(root);
  scene.add(holder);

  // Stand the model on the floor, centred.
  const bounds = new Box3().setFromObject(root);
  const size = bounds.getSize(new Vector3()), center = bounds.getCenter(new Vector3());
  root.position.sub(new Vector3(center.x, bounds.min.y, center.z));
  const radius = Math.max(size.length() / 2, 1e-6);
  const grid = new GridHelper(radius * 4, 20, new Color(0x3a4a41), new Color(0x243028));
  scene.add(grid);
  key.position.set(radius * 2, radius * 3, radius * 2);
  fill.position.set(-radius * 2, radius, -radius * 1.5);

  const camera = new PerspectiveCamera(40, 1, radius / 100, radius * 100);
  const controls = new OrbitControls(camera, renderer.domElement);
  controls.enableDamping = false;
  const target = new Vector3(0, size.y / 2, 0);

  let triangles = 0, meshes = 0;
  root.traverse(o => {
    if (!(o instanceof Mesh)) return;
    meshes++;
    const g = o.geometry;
    triangles += (g.index ? g.index.count : g.attributes.position?.count ?? 0) / 3;
  });

  const draw = () => renderer.render(scene, camera);
  const resize = () => {
    const w = box.clientWidth, h = box.clientHeight;
    if (!w || !h) return;
    renderer.setSize(w, h, false);
    camera.aspect = w / h;
    camera.updateProjectionMatrix();
    draw();
  };
  const reset = () => {
    const fov = (camera.fov * Math.PI) / 180;
    const dist = radius / Math.sin(fov / 2) * 1.1;
    camera.position.copy(target).add(new Vector3(1, 0.7, 1.2).normalize().multiplyScalar(dist));
    controls.target.copy(target);
    controls.update();
    draw();
  };
  // Drawn only when something moves: nothing spins in the background.
  controls.addEventListener("change", draw);
  const watch = new ResizeObserver(resize);
  watch.observe(box);
  resize();
  reset();

  return {
    info: { format, triangles: Math.round(triangles), meshes },
    reset,
    resize,
    wireframe(on) {
      root.traverse(o => {
        if (!(o instanceof Mesh)) return;
        for (const m of Array.isArray(o.material) ? o.material : [o.material]) if ("wireframe" in m) m.wireframe = on;
      });
      draw();
    },
    dispose() {
      watch.disconnect();
      controls.dispose();
      root.traverse(o => {
        if (!(o instanceof Mesh)) return;
        o.geometry.dispose();
        for (const m of Array.isArray(o.material) ? o.material : [o.material]) {
          for (const v of Object.values(m)) if (v instanceof Texture) v.dispose();
          m.dispose();
        }
      });
      renderer.dispose();
      renderer.domElement.remove();
    },
  };
}
