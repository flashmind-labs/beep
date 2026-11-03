// The media pane's 3D viewer: a glTF, GLB, OBJ or STL model the agent
// made, drawn with three.js, framed to fit, lit simply, turned with the
// pointer. Loaded only when a model is first opened, so three.js stays
// out of the window's start.

import {
  AmbientLight, type AnimationClip, AnimationMixer, Box3, Color, DirectionalLight, GridHelper, Group, HemisphereLight,
  LoopOnce, LoopRepeat, Mesh, MeshStandardMaterial, type Object3D, PerspectiveCamera, Scene, SRGBColorSpace, Texture,
  Vector3, WebGLRenderer,
} from "three";
import { clipName } from "./media-state.ts";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { OBJLoader } from "three/examples/jsm/loaders/OBJLoader.js";
import { STLLoader } from "three/examples/jsm/loaders/STLLoader.js";

/// What the viewer says of a model under it.
export interface ModelInfo {
  format: string;
  triangles: number;
  meshes: number;
  clips: number;
}

/// A glTF's animation: its clips, the one shown, and where it is.
export interface Animation {
  clips: { name: string; duration: number }[];
  clip: number;
  playing: boolean;
  loop: boolean;
  time(): number;
  select(clip: number): void;
  play(on: boolean): void;
  seek(time: number): void;
  setLoop(on: boolean): void;
  /// Called on each change: each frame it plays, a stop at its end, and
  /// each call above.
  onchange: () => void;
}

export interface ModelView {
  info: ModelInfo;
  /// Frames the whole model again.
  reset(): void;
  wireframe(on: boolean): void;
  /// Null when the model has no animation.
  anim: Animation | null;
  /// Draws again after the box changed size.
  resize(): void;
  dispose(): void;
}

/// Reads a file a `.gltf` names next to it (a buffer or texture), by its
/// relative URI; the bytes, or null when it cannot be read.
export type Sibling = (uri: string) => Promise<Uint8Array | null>;

const gray = () => new MeshStandardMaterial({ color: 0xb8c4bd, roughness: 0.6, metalness: 0.05 });

/// Parses `bytes` of a model file of extension `ext` into a scene node.
async function parse(bytes: ArrayBuffer, ext: string, sibling: Sibling): Promise<{ root: Object3D; format: string; clips: AnimationClip[] }> {
  if (ext === "stl") {
    // The loader keeps the file's own face normals.
    return { root: new Mesh(new STLLoader().parse(bytes), gray()), format: "STL", clips: [] };
  }
  if (ext === "obj") {
    const root = new OBJLoader().parse(new TextDecoder().decode(bytes));
    // An OBJ's materials live in an .mtl the viewer does not read.
    const material = gray();
    root.traverse(o => { if (o instanceof Mesh) o.material = material; });
    return { root, format: "OBJ", clips: [] };
  }
  const glb = ext === "gltf" ? await packGltf(bytes, sibling) : bytes;
  const loader = new GLTFLoader();
  // Pictures decode off the main thread with createImageBitmap, from
  // their bytes: three's own bitmap loader fetches a blob: URL, which the
  // window's content policy refuses.
  loader.register(parser => {
    const own = parser.loadImageSource.bind(parser);
    const cache: Record<number, Promise<Texture>> = (parser as unknown as { sourceCache: Record<number, Promise<Texture>> }).sourceCache;
    parser.loadImageSource = (i, imageLoader) => {
      const def = parser.json.images[i];
      if (def.bufferView === undefined) return own(i, imageLoader);
      if (i in cache) return cache[i].then(t => t.clone());
      return cache[i] = parser.getDependency("bufferView", def.bufferView)
        .then(view => createImageBitmap(new Blob([view], { type: def.mimeType }), { premultiplyAlpha: "none" }))
        .then(bitmap => {
          const t = new Texture(bitmap);
          t.userData.mimeType = def.mimeType;
          t.needsUpdate = true;
          return t;
        });
    };
    return { name: "beep_bitmaps" };
  });
  const gltf = await loader.parseAsync(glb, "");
  return { root: gltf.scene, format: ext === "gltf" ? "glTF" : "GLB", clips: gltf.animations };
}

const b64 = (s: string) => Uint8Array.from(atob(s), c => c.charCodeAt(0));
const pad4 = (n: number) => (n + 3) & ~3;

/// A `.gltf` and the files it names as one GLB, so nothing is fetched by
/// URL: its buffers and pictures (beside it or in data URIs) joined into
/// the binary chunk.
async function packGltf(bytes: ArrayBuffer, sibling: Sibling): Promise<ArrayBuffer> {
  const json = JSON.parse(new TextDecoder().decode(bytes));
  const buffers: { uri?: string; byteLength: number }[] = json.buffers ?? [];
  const parts: Uint8Array[] = [];
  const base: number[] = [];
  let length = 0;
  const read = (uri: string) => uri.startsWith("data:") ? b64(uri.slice(uri.indexOf(",") + 1)) : sibling(uri);
  const add = (data: Uint8Array) => {
    base.push(length);
    parts.push(data);
    length = pad4(length + data.length);
  };
  for (const b of buffers) {
    const data = b.uri ? await read(b.uri) : new Uint8Array(b.byteLength);
    if (!data) throw new Error(`${b.uri} is not there`);
    add(data);
  }
  json.bufferViews ??= [];
  for (const v of json.bufferViews) {
    v.byteOffset = (v.byteOffset ?? 0) + base[v.buffer];
    v.buffer = 0;
  }
  for (const img of json.images ?? []) {
    const data = img.uri && await read(img.uri);
    if (!data) continue;
    img.bufferView = json.bufferViews.push({ buffer: 0, byteOffset: length, byteLength: data.length }) - 1;
    delete img.uri;
    add(data);
  }
  json.buffers = length ? [{ byteLength: length }] : [];
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
  const { root, format, clips } = await parse(bytes, ext, sibling);
  // A retina screen's pixels are fine enough without multisampling, which
  // would cost four samples on each of them.
  const renderer = new WebGLRenderer({ antialias: devicePixelRatio < 2, alpha: true });
  renderer.setPixelRatio(Math.min(2, devicePixelRatio));
  renderer.outputColorSpace = SRGBColorSpace;
  box.append(renderer.domElement);

  const scene = new Scene();
  // Nothing moves but the camera and an animation, which update the
  // matrices themselves, so a frame does not walk every node for them.
  scene.matrixWorldAutoUpdate = false;
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
  scene.updateMatrixWorld();

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

  const mixer = clips.length ? new AnimationMixer(root) : null;
  const actions = mixer ? clips.map(c => mixer.clipAction(c)) : [];
  let anim: Animation | null = null;

  // One draw per frame at most, and only when something moved: an orbit,
  // a resize, a reset, or a playing animation.
  let raf = 0, last = 0;
  const draw = () => renderer.render(scene, camera);
  const frame = (now: number) => {
    raf = 0;
    if (mixer && anim?.playing) {
      mixer.update(last ? (now - last) / 1000 : 0);
      last = now;
      root.updateMatrixWorld();
      anim.onchange();
      if (anim.playing) raf = requestAnimationFrame(frame);
    }
    draw();
  };
  const request = () => { raf ||= requestAnimationFrame(frame); };
  const resize = () => {
    const w = box.clientWidth, h = box.clientHeight;
    if (!w || !h) return;
    renderer.setSize(w, h, false);
    camera.aspect = w / h;
    camera.updateProjectionMatrix();
    // Now, not next frame: setSize has just cleared the canvas.
    draw();
  };
  const reset = () => {
    const fov = (camera.fov * Math.PI) / 180;
    const dist = radius / Math.sin(fov / 2) * 1.1;
    camera.position.copy(target).add(new Vector3(1, 0.7, 1.2).normalize().multiplyScalar(dist));
    controls.target.copy(target);
    controls.update();
    request();
  };

  if (mixer) {
    const pose = () => {
      mixer.update(0);
      root.updateMatrixWorld();
      request();
    };
    const a: Animation = anim = {
      clips: clips.map((c, i) => ({ name: clipName(c.name, i), duration: c.duration })),
      clip: 0,
      playing: false,
      loop: true,
      time: () => actions[a.clip].time,
      select(i) {
        actions[a.clip].stop();
        a.clip = i;
        a.setLoop(a.loop);
        actions[i].play();
        actions[i].paused = !a.playing;
        pose();
        a.onchange();
      },
      play(on) {
        const action = actions[a.clip];
        if (on && !a.loop && action.time >= clips[a.clip].duration) action.time = 0;
        action.enabled = true;
        action.paused = !on;
        a.playing = on;
        last = 0;
        request();
        a.onchange();
      },
      seek(time) {
        actions[a.clip].time = time;
        actions[a.clip].enabled = true;
        pose();
        a.onchange();
      },
      setLoop(on) {
        a.loop = on;
        actions[a.clip].setLoop(on ? LoopRepeat : LoopOnce, Infinity);
        actions[a.clip].clampWhenFinished = true;
        a.onchange();
      },
      onchange: () => {},
    };
    // Played through once, it holds its last pose.
    mixer.addEventListener("finished", () => {
      a.playing = false;
      a.onchange();
    });
    a.select(0);
  }

  // Shaders compile in parallel where the GPU allows, before the first
  // frame needs them.
  await renderer.compileAsync(scene, camera);
  controls.addEventListener("change", request);
  const watch = new ResizeObserver(resize);
  watch.observe(box);
  resize();
  reset();

  return {
    info: { format, triangles: Math.round(triangles), meshes, clips: clips.length },
    reset,
    resize,
    anim,
    wireframe(on) {
      root.traverse(o => {
        if (!(o instanceof Mesh)) return;
        for (const m of Array.isArray(o.material) ? o.material : [o.material]) if ("wireframe" in m) m.wireframe = on;
      });
      request();
    },
    dispose() {
      cancelAnimationFrame(raf);
      watch.disconnect();
      controls.dispose();
      mixer?.stopAllAction();
      mixer?.uncacheRoot(root);
      root.traverse(o => {
        if (!(o instanceof Mesh)) return;
        o.geometry.dispose();
        for (const m of Array.isArray(o.material) ? o.material : [o.material]) {
          for (const v of Object.values(m)) {
            if (!(v instanceof Texture)) continue;
            v.dispose();
            if (v.image instanceof ImageBitmap) v.image.close();
          }
          m.dispose();
        }
      });
      grid.dispose();
      renderer.dispose();
      // Frees the GL context now: the window holds only a few at once.
      renderer.forceContextLoss();
      renderer.domElement.remove();
    },
  };
}
