// Scenes painted in short brush strokes, after Monet: a sunrise over a
// harbour ("Impression, Sunrise"), the dawn haze of its sky alone, haystacks at dusk, the lily
// pond under its footbridge and Parliament in the evening fog, and after Friedrich a
// wanderer on a rock above a sea of fog. Each is an SVG file in `scenes/`, drawn in any vector
// editor; the strokes take their colours from it. In a scene's file:
// - `class="glint"`: shown only in the strokes that catch the light.
// - `class="ink"`, on a child of the root: drawn sharp over the strokes as the painting fills in.
// - `class="rise"`, on a child of the root: climbs `data-rise` of the height into the place
//   drawn as the painting fills in, shown only above `data-level`.
// - `data-level` on the root: where water or open ground begins, down the height (0 to 1).
//   Strokes lie level below it and rows sway widest there.
// - `preserveAspectRatio="none"` on the root stretches it to the canvas instead of cropping it.
// A scene paints
// itself once after the app opens. From then on it stays alive while it is on screen: at each
// beat its rows of pixels sway, most on water and open ground, and a few short runs of them take
// a new light. The dawn haze lies behind lists and stays still, and with Reduce motion on every
// scene is drawn finished and still.
import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import haystacks from "./scenes/haystacks.svg";
import haze from "./scenes/haze.svg";
import lilies from "./scenes/lilies.svg";
import parliament from "./scenes/parliament.svg";
import sunrise from "./scenes/sunrise.svg";
import wanderer from "./scenes/wanderer.svg";

/// The sunrise: it rises once after the app opens, wherever it shows first.
export function SunriseArt() {
  return <PaintedArt scene="sunrise" />;
}

/// The dawn haze, which fills in stroke by stroke the first time it shows.
export function HazeArt() {
  return <PaintedArt scene="haze" />;
}

/// The wanderer above the sea of fog, which fills in stroke by stroke the first time it shows.
export function WandererArt() {
  return <PaintedArt scene="wanderer" />;
}

/// The day's painting: the sunrise, the haystacks, the lily pond and Parliament take a day each.
/// The day is read when the screen opens, so a painting on show never changes.
export function DailyArt() {
  const [scene] = useState(() => DAILY[Math.floor((Date.now() - new Date().getTimezoneOffset() * 60_000) / 86_400_000) % DAILY.length]);
  return <PaintedArt scene={scene} />;
}

/// An empty screen: the day's painting, with what to do next on a card over its quiet side.
export function PaintedEmpty({ children }: { children: ReactNode }) {
  return <div className="painted-empty"><DailyArt /><div className="cc-panel cc-empty">{children}</div></div>;
}

type Scene = "sunrise" | "haze" | "wanderer" | "haystacks" | "lilies" | "parliament";
const DAILY: readonly Scene[] = ["sunrise", "haystacks", "lilies", "parliament"];
/// A project's own painting, one of the scenes picked by its name, so projects differ and each
/// keeps the same one.
export function ProjectArt({ project }: { project: string }) {
  const at = [...project].reduce((hash, char) => (hash * 31 + char.charCodeAt(0)) >>> 0, 7);
  return <PaintedArt key={project} scene={PROJECT_SCENES[at % PROJECT_SCENES.length]} />;
}
const PROJECT_SCENES: readonly Scene[] = ["sunrise", "haystacks", "lilies", "parliament", "wanderer"];

/// When each scene began to paint itself. A scene drawn again, on another
/// screen or after a resize, goes on from there instead of starting over.
const began = new Map<Scene, number>();

function PaintedArt({ scene }: { scene: Scene }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  useLayoutEffect(() => {
    const target = canvas.current;
    if (!target) return;

    // `life` is the finished painting, kept to move it; it is dropped when the strokes are laid again.
    // A finished painting beats on a timer, so the page wakes once a beat instead of on every display frame.
    // `picture` is the scene's SVG drawn at the canvas's size; nothing is painted before it.
    let frame = 0, timer = 0, strokes: Stroke[] = [], last = 0, seen = false, life: Life | null = null, picture: Picture | null = null, gone = false;
    const still = typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const alive = !still && scene !== "haze";
    const paint = (now: number) => {
      frame = 0;
      if (!picture) return;
      if (!began.has(scene)) began.set(scene, now);
      const done = still ? 1 : Math.min(1, (now - (began.get(scene) ?? now)) / PLAY_MS[scene]);

      if (life) {
        beat(target, life, now);
      } else if (done === 1 || now - last >= FRAME_MS) {
        last = now;
        draw(target, strokes, done, picture);
        if (done === 1 && alive) life = hold(target, scene, picture);
      }

      if (done < 1) frame = requestAnimationFrame(paint);
      else if (life && seen && !document.hidden) timer = window.setTimeout(wake, BEAT_MS);
    };
    function wake() {
      window.clearTimeout(timer);
      timer = 0;
      if (!frame) frame = requestAnimationFrame(paint);
    }

    const resized = new ResizeObserver(() => {
      const width = target.clientWidth, height = target.clientHeight;
      life = null;
      picture = null;
      void load(scene).then(layers => {
        if (gone || !layers || width !== target.clientWidth || height !== target.clientHeight) return;
        picture = rasterize(layers, width, height);
        strokes = layStrokes(width, height, scene, picture);
        wake();
      });
    });
    resized.observe(target);
    // A canvas that only touches the edge of its scroller is reported as crossing it with no area:
    // the second threshold tells that apart from one that comes into view.
    const watch = new IntersectionObserver(entries => {
      seen = entries[entries.length - 1].intersectionRatio > 0;
      if (seen) wake();
    }, { threshold: [0, 0.001] });
    const shown = () => { if (!document.hidden) wake(); };
    if (alive) {
      watch.observe(target);
      document.addEventListener("visibilitychange", shown);
    }

    return () => {
      gone = true;
      resized.disconnect();
      watch.disconnect();
      document.removeEventListener("visibilitychange", shown);
      if (frame) cancelAnimationFrame(frame);
      window.clearTimeout(timer);
    };
  }, [scene]);

  return <canvas ref={canvas} className={`painted-art ${scene}`} aria-hidden="true" />;
}

/// How long each scene takes to paint itself, and the time between two painted frames.
const PLAY_MS: Record<Scene, number> = { sunrise: 9000, haze: 2600, wanderer: 4200, haystacks: 4200, lilies: 4200, parliament: 4200 }, FRAME_MS = 33;
/// A living painting: the time between two beats, the share of its pixels that take a new light at
/// each, the longest run that changes together and how far its light moves either way.
const BEAT_MS = 100, BEAT_SHARE = 0.03, RUN = 4, SWING = 0.13;
/// How far a row sways to either side, in painted pixels: everywhere, and at the near edge of water
/// and open ground, where the waves are widest.
const SWAY = 1.1, SWAY_NEAR = 3;
/// The brush of each scene. The haze is a narrow picture and the others have small shapes: they take a finer one.
const BRUSH: Record<Scene, number> = { sunrise: 1, haze: 0.55, wanderer: 0.7, haystacks: 0.65, lilies: 0.7, parliament: 0.65 };
/// The side of one painted pixel in CSS pixels: the scenes are painted in blocks.
const PIXEL = 4;
const FILES: Record<Scene, string> = { sunrise, haze, wanderer, haystacks, lilies, parliament };

type Rgb = readonly [number, number, number];
/// One brush stroke: its middle, size and tilt, and the chance values that
/// shift its colour (`tint`) and decide whether it catches the light (`glint`).
interface Stroke { x: number; y: number; length: number; width: number; tilt: number; tint: number; glint: number }

/// A scene's file decoded once in each of its layers: with its glints and without, what rises
/// and what is drawn sharp, and the root's settings.
interface Layers { lit: HTMLImageElement; plain: HTMLImageElement; rise: HTMLImageElement | null; ink: HTMLImageElement | null; stretch: boolean; level: number; climb: number }
/// The layers drawn at a canvas's size.
interface Picture { lit: ImageData; plain: ImageData; rise: ImageData | null; ink: HTMLCanvasElement | null; level: number; climb: number }

const loaded = new Map<Scene, Promise<Layers | null>>();

/// A scene's layers, decoded on first use. Null when the browser cannot draw its file.
function load(scene: Scene): Promise<Layers | null> {
  const known = loaded.get(scene);
  if (known) return known;

  const layers = decode(FILES[scene]).catch(() => null);
  loaded.set(scene, layers);
  return layers;
}

async function decode(svg: string): Promise<Layers> {
  const root = new DOMParser().parseFromString(svg, "image/svg+xml").documentElement;
  const has = (name: string) => root.querySelector(`:scope > .${name}`) !== null;
  const number = (name: string, missing: number) => {
    const value = Number(root.getAttribute(name) ?? missing);
    return Number.isFinite(value) ? value : missing;
  };
  const variant = (css: string) => {
    const copy = root.cloneNode(true);
    if (!(copy instanceof Element)) throw new Error("not an SVG element");
    const style = copy.ownerDocument.createElementNS("http://www.w3.org/2000/svg", "style");
    style.textContent = css;
    copy.prepend(style);
    const image = new Image();
    image.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(new XMLSerializer().serializeToString(copy))}`;
    return image.decode().then(() => image);
  };
  const only = (name: string) => has(name) ? variant(`:scope > :not(defs, style, .${name}) { display: none }`) : null;

  const [lit, plain, rise, ink] = await Promise.all([
    variant(".ink, .rise { display: none }"),
    variant(".ink, .rise, .glint { display: none }"),
    only("rise"),
    only("ink"),
  ]);
  return { lit, plain, rise, ink, stretch: root.getAttribute("preserveAspectRatio") === "none", level: number("data-level", 1), climb: number("data-rise", 0) };
}

/// The layers drawn to fill a canvas of this size: stretched, or cropped to the middle like a cover.
function rasterize(layers: Layers, width: number, height: number): Picture {
  // Sampled at half the canvas's size: the strokes are wider than that.
  const w = Math.max(1, Math.round(width / 2)), h = Math.max(1, Math.round(height / 2));
  const { naturalWidth, naturalHeight } = layers.lit;
  const scale = Math.max(w / naturalWidth, h / naturalHeight);
  const [sx, sy, sw, sh] = layers.stretch ? [0, 0, naturalWidth, naturalHeight] : [(naturalWidth - w / scale) / 2, (naturalHeight - h / scale) / 2, w / scale, h / scale];
  const canvasOf = (image: HTMLImageElement) => {
    const canvas = document.createElement("canvas");
    canvas.width = w;
    canvas.height = h;
    canvas.getContext("2d", { willReadFrequently: true })?.drawImage(image, sx, sy, sw, sh, 0, 0, w, h);
    return canvas;
  };
  const pixels = (image: HTMLImageElement) => canvasOf(image).getContext("2d", { willReadFrequently: true })?.getImageData(0, 0, w, h) ?? new ImageData(w, h);

  return {
    lit: pixels(layers.lit),
    plain: pixels(layers.plain),
    rise: layers.rise && pixels(layers.rise),
    ink: layers.ink && canvasOf(layers.ink),
    level: layers.level,
    climb: layers.climb,
  };
}

/// A repeatable sequence of numbers from 0 to 1, so the painting is the same each time.
function sequence(seed: number) {
  return () => {
    seed = seed + 0x6d2b79f5 | 0;
    let t = Math.imul(seed ^ seed >>> 15, 1 | seed);
    t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t;
    return ((t ^ t >>> 14) >>> 0) / 4294967296;
  };
}

/// The strokes for a canvas of this size: broad ones that cover it, then finer ones over them.
/// They lie level on water and open ground.
function layStrokes(width: number, height: number, scene: Scene, picture: Picture): Stroke[] {
  const next = sequence(7), strokes: Stroke[] = [], brush = BRUSH[scene];
  const add = (x: number, y: number, length: number, thick: number) => {
    strokes.push({ x, y, length: length * brush * (0.7 + next() * 0.6), width: thick * brush * (0.7 + next() * 0.6), tilt: (next() - 0.5) * (y / height > picture.level ? 0.12 : 0.5), tint: next(), glint: next() });
  };

  for (let y = 0; y < height + 10; y += 10 * brush) {
    for (let x = 0; x < width + 26; x += 26 * brush) add(x + (next() - 0.5) * 20 * brush, y + (next() - 0.5) * 8 * brush, 40, 13);
  }
  for (let i = Math.round(width * height / (260 * brush * brush)); i > 0; i--) add(next() * width, next() * height, 18, 5);

  return strokes;
}

/// The colour of a picture at a point (both from 0 to 1), with what rises `lift` of the height
/// below its place.
function colourAt(picture: Picture, u: number, v: number, glint: number, lift: number): Rgb {
  const base = glint > 0.5 ? picture.lit : picture.plain, i = at(base, u, v);
  const colour: Rgb = [base.data[i], base.data[i + 1], base.data[i + 2]];
  if (!picture.rise || v >= picture.level) return colour;

  const j = at(picture.rise, u, v - lift), over = picture.rise.data, a = over[j + 3] / 255;
  return [colour[0] + (over[j] - colour[0]) * a, colour[1] + (over[j + 1] - colour[1]) * a, colour[2] + (over[j + 2] - colour[2]) * a];
}

/// Where a point (both from 0 to 1) is in the pixels of an image.
function at(data: ImageData, u: number, v: number): number {
  const x = Math.min(data.width - 1, Math.max(0, Math.floor(u * data.width))), y = Math.min(data.height - 1, Math.max(0, Math.floor(v * data.height)));
  return (y * data.width + x) * 4;
}

function draw(target: HTMLCanvasElement, strokes: Stroke[], done: number, picture: Picture) {
  const context = target.getContext("2d");
  const width = target.clientWidth, height = target.clientHeight;
  if (!context || !width || !height) return;

  const scale = 1 / PIXEL;
  if (target.width !== Math.round(width * scale) || target.height !== Math.round(height * scale)) {
    target.width = Math.round(width * scale);
    target.height = Math.round(height * scale);
  }
  context.setTransform(scale, 0, 0, scale, 0, 0);
  context.lineCap = "round";

  // What rises slows as it climbs and the light comes up with it; other scenes are lit evenly.
  const rise = 1 - (1 - done) ** 3, light = picture.rise ? 0.5 + 0.5 * rise : 0.95, lift = picture.climb * (1 - rise);
  const paint = (colour: Rgb, shade: number) => `rgb(${Math.round(colour[0] * shade)},${Math.round(colour[1] * shade)},${Math.round(colour[2] * shade)})`;

  // An underpainting, so no gap between strokes shows the page.
  for (let y = 0; y < height; y += 6) {
    context.fillStyle = paint(colourAt(picture, 0.05, y / height, 0, lift), light * 0.8);
    context.fillRect(0, y, width, 6);
  }

  // A scene with something rising is repainted whole as it moves; the others fill in stroke by stroke.
  const shown = picture.rise ? strokes.length : Math.round(strokes.length * done);
  for (let i = 0; i < shown; i++) {
    const s = strokes[i], dx = Math.cos(s.tilt) * s.length / 2, dy = Math.sin(s.tilt) * s.length / 2;
    context.strokeStyle = paint(colourAt(picture, s.x / width, s.y / height, s.glint, lift), light * (0.86 + s.tint * 0.28));
    context.lineWidth = s.width;
    context.beginPath();
    context.moveTo(s.x - dx, s.y - dy);
    context.lineTo(s.x + dx, s.y + dy);
    context.stroke();
  }

  if (picture.ink) {
    context.globalAlpha = done;
    context.drawImage(picture.ink, 0, 0, width, height);
    context.globalAlpha = 1;
  }
}

/// A finished painting kept to move it: its own pixels, the ones on the canvas, each pixel's light
/// (128 is its own) and how far each row sways.
interface Life { base: Uint8ClampedArray; live: ImageData; glow: Uint8Array; reach: Float32Array }

function hold(target: HTMLCanvasElement, scene: Scene, picture: Picture): Life | null {
  const context = target.getContext("2d"), width = target.width, height = target.height;
  if (!context || !width || !height) return null;

  const base = context.getImageData(0, 0, width, height).data;
  // Waves widen toward the viewer from where the water or the ground begins. The wanderer stands
  // in his fog, so it only sways as the rest does.
  let shore = height;
  if (scene !== "wanderer") for (let y = height - 1; y >= 0 && y / height > picture.level; y--) shore = y;

  return {
    base,
    live: new ImageData(new Uint8ClampedArray(base), width, height),
    glow: new Uint8Array(width * height).fill(128),
    reach: Float32Array.from({ length: height }, (_, y) => y < shore ? SWAY : SWAY + (SWAY_NEAR - SWAY) * Math.min(1, (y - shore) / (height * 0.3))),
  };
}

/// One beat: a few runs of pixels take a new light, then every row is laid where it has swayed to.
function beat(target: HTMLCanvasElement, { base, live, glow, reach }: Life, now: number) {
  const width = live.width, height = live.height, pixels = live.data, count = width * height, time = now / 1000;
  for (let runs = Math.round(count * BEAT_SHARE / (RUN / 2 + 0.5)); runs > 0; runs--) {
    const from = Math.floor(Math.random() * count), to = Math.min(count, from + 1 + Math.floor(Math.random() * RUN));
    glow.fill(128 * (1 + (Math.random() * 2 - 1) * SWING), from, to);
  }

  // Short waves on the water, a long slow one over everything.
  for (let y = 0, i = 0; y < height; y++) {
    const shift = Math.round((reach[y] - SWAY) * Math.sin(y * 0.6 + time * 2.6) + SWAY * Math.sin(y * 0.11 - time * 0.9));
    for (let x = 0; x < width; x++, i++) {
      const from = (y * width + Math.min(width - 1, Math.max(0, x + shift))) * 4, light = glow[i] / 128, to = i * 4;
      pixels[to] = base[from] * light;
      pixels[to + 1] = base[from + 1] * light;
      pixels[to + 2] = base[from + 2] * light;
    }
  }

  target.getContext("2d")?.putImageData(live, 0, 0);
}
