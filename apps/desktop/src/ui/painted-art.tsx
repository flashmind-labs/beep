// Scenes painted in short brush strokes, after Monet: a sunrise over a
// harbour ("Impression, Sunrise"), the dawn haze of its sky alone, haystacks at dusk, the lily
// pond under its footbridge and Parliament in the evening fog, and after Friedrich a
// wanderer on a rock above a sea of fog. A scene paints
// itself once after the app opens. From then on it stays alive while it is on screen: at each
// beat its rows of pixels sway, most on water and open ground, and a few short runs of them take
// a new light. The dawn haze lies behind lists and stays still, and with Reduce motion on every
// scene is drawn finished and still.
import { useLayoutEffect, useRef, useState, type ReactNode } from "react";

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
/// When each scene began to paint itself. A scene drawn again, on another
/// screen or after a resize, goes on from there instead of starting over.
const began = new Map<Scene, number>();

function PaintedArt({ scene }: { scene: Scene }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  useLayoutEffect(() => {
    const target = canvas.current;
    if (!target) return;

    // `life` is the finished painting, kept to move it; it is dropped when the strokes are laid again.
    let frame = 0, strokes: Stroke[] = [], last = 0, seen = false, life: Life | null = null;
    const still = typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const alive = !still && scene !== "haze";
    const paint = (now: number) => {
      frame = 0;
      if (!began.has(scene)) began.set(scene, now);
      const done = still ? 1 : Math.min(1, (now - (began.get(scene) ?? now)) / PLAY_MS[scene]);

      if (life) {
        if (now - last >= BEAT_MS) {
          last = now;
          beat(target, life, now);
        }
      } else if (done === 1 || now - last >= FRAME_MS) {
        last = now;
        draw(target, strokes, done, scene);
        if (done === 1 && alive) life = hold(target, scene);
      }

      if (done < 1 || (life && seen && !document.hidden)) frame = requestAnimationFrame(paint);
    };
    const wake = () => { if (!frame) frame = requestAnimationFrame(paint); };

    const resized = new ResizeObserver(() => {
      strokes = layStrokes(target.clientWidth, target.clientHeight, scene);
      life = null;
      wake();
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
      resized.disconnect();
      watch.disconnect();
      document.removeEventListener("visibilitychange", shown);
      if (frame) cancelAnimationFrame(frame);
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
/// Where the water meets the sky, and where the sun stands across the width.
const HORIZON = 0.58, SUN_X = 0.57;

type Rgb = readonly [number, number, number];
/// One brush stroke: its middle, size and tilt, and the chance values that
/// shift its colour (`tint`) and decide whether it catches the sun (`glint`).
interface Stroke { x: number; y: number; length: number; width: number; tilt: number; tint: number; glint: number }

const SKY_HIGH: Rgb = [50, 64, 92], SKY_LOW: Rgb = [124, 120, 134], HAZE: Rgb = [186, 124, 110], GLOW: Rgb = [222, 132, 92];
const SUN: Rgb = [244, 100, 46], WATER_FAR: Rgb = [88, 102, 120], WATER_NEAR: Rgb = [38, 56, 78], BOAT: Rgb = [20, 32, 46], MAST: Rgb = [66, 80, 102];

const FOG_SKY: Rgb = [200, 190, 172], FOG_HIGH: Rgb = [112, 130, 154], FOG: Rgb = [192, 190, 186], PEAK: Rgb = [66, 82, 108], RIDGE: Rgb = [52, 50, 56];
const ROCK: Rgb = [34, 28, 28], ROCK_LIT: Rgb = [84, 66, 56], COAT: Rgb = [14, 26, 24], COAT_LIT: Rgb = [28, 46, 40], LEGS: Rgb = [24, 34, 32], CANE: Rgb = [96, 70, 48], SKIN: Rgb = [150, 112, 92], HAIR: Rgb = [176, 112, 58];
/// Where the wanderer stands: across the width, and the top of the rock under his feet.
const WANDERER_X = 0.5, ROCK_TOP = 0.74;

const DUSK_HIGH: Rgb = [56, 62, 100], DUSK_LOW: Rgb = [150, 98, 100], AFTERGLOW: Rgb = [236, 150, 84], HILLS: Rgb = [70, 66, 100], HEDGE: Rgb = [44, 50, 66];
const FIELD_FAR: Rgb = [150, 104, 84], FIELD_NEAR: Rgb = [60, 54, 66], STACK: Rgb = [62, 32, 48], THATCH: Rgb = [146, 76, 50], STACK_LIT: Rgb = [222, 124, 62], STACK_SHADOW: Rgb = [44, 42, 74];
/// Where the field meets the hills, and the two stacks: each one's place across the width, its
/// foot, and its half width and height in the picture's height.
const FIELD = 0.4, STACKS = [[0.64, 0.465, 0.155, 0.27], [0.515, 0.44, 0.09, 0.15]] as const;

const LEAVES: Rgb = [26, 50, 44], LEAVES_LIT: Rgb = [92, 126, 80], WILLOW: Rgb = [142, 150, 88], BRIDGE: Rgb = [128, 170, 154], BRIDGE_SHADE: Rgb = [58, 98, 98];
const POND_DARK: Rgb = [22, 44, 56], POND_GREEN: Rgb = [50, 88, 70], POND_SKY: Rgb = [92, 108, 142], PAD: Rgb = [118, 154, 104], PAD_SHADE: Rgb = [62, 104, 84], BLOOM: Rgb = [232, 150, 172], BLOOM_PALE: Rgb = [238, 228, 214];
/// Where the pond begins under the trees, and how far the footbridge reaches to each side of the middle.
const POND = 0.43, BRIDGE_SPAN = 0.4;

const EVENING: Rgb = [70, 62, 108], EMBER: Rgb = [170, 96, 88], GOLD: Rgb = [240, 166, 84], LOW_SUN: Rgb = [252, 206, 112], TOWERS: Rgb = [52, 54, 102];
const THAMES_FAR: Rgb = [150, 96, 96], THAMES_NEAR: Rgb = [40, 40, 82];
/// Where the river meets the far bank, where the evening sun stands, and the skyline: each
/// tower's place across the width, its half width in the picture's height, its top and how
/// far its sides fall away from the point. One with no point has a flat roof between turrets.
const THAMES = 0.57, LOW_SUN_X = 0.46, LOW_SUN_V = 0.28;
const SKYLINE = [[0.68, 0.085, 0.15, 0], [0.59, 0.014, 0.26, 0.13], [0.83, 0.03, 0.21, 0.08], [0.535, 0.02, 0.36, 0.06]] as const;

const mix = (a: Rgb, b: Rgb, k: number): Rgb => [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k];

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
function layStrokes(width: number, height: number, scene: Scene): Stroke[] {
  const next = sequence(7), strokes: Stroke[] = [], brush = BRUSH[scene];
  const add = (x: number, y: number, length: number, thick: number) => {
    strokes.push({ x, y, length: length * brush * (0.7 + next() * 0.6), width: thick * brush * (0.7 + next() * 0.6), tilt: (next() - 0.5) * (level(scene, y / height) ? 0.12 : 0.5), tint: next(), glint: next() });
  };

  for (let y = 0; y < height + 10; y += 10 * brush) {
    for (let x = 0; x < width + 26; x += 26 * brush) add(x + (next() - 0.5) * 20 * brush, y + (next() - 0.5) * 8 * brush, 40, 13);
  }
  for (let i = Math.round(width * height / (260 * brush * brush)); i > 0; i--) add(next() * width, next() * height, 18, 5);

  return strokes;
}

/// Whether the strokes at this height (from 0 to 1) lie level, as on water and open ground.
function level(scene: Scene, v: number): boolean {
  switch (scene) {
    case "sunrise": return v > HORIZON;
    case "haze": return false;
    case "wanderer": return v > 0.44 && v < ROCK_TOP;
    case "haystacks": return v > FIELD;
    case "lilies": return v > POND;
    case "parliament": return v > THAMES;
  }
}

/// The colour of the haze at a point (both from 0 to 1): the sunrise's sky, warm in its top corner.
function haze(u: number, v: number, glint: number): Rgb {
  const corner = Math.hypot(u * 0.5, v * 1.6);
  return mix(mix(SKY_LOW, SKY_HIGH, Math.min(1, v * 1.2 + glint * 0.2)), GLOW, Math.exp(-corner * corner / 0.12) * 0.8);
}

/// A hill's top at `u`: a point `tall` above `foot`, `wide` to each side, with a rough edge.
const hill = (u: number, at: number, foot: number, tall: number, wide: number) => foot - tall * Math.max(0, 1 - Math.abs(u - at) / wide) - 0.007 * Math.sin(u * 61 + at * 9) - 0.005 * Math.sin(u * 173);

/// The colour of the wanderer's view at a point (both from 0 to 1): sky, far mountains, the fog
/// with ridges running down to the middle of it, and the dark crag he stands on.
function wanderer(u: number, v: number, aspect: number, glint: number): Rgb {
  // The crag climbs slowly from the left and drops steeply to the right, where a lower one stands.
  const across = (u - WANDERER_X) * aspect, rough = 0.014 * Math.sin(u * 53) + 0.009 * Math.sin(u * 131 + 1);
  const crag = Math.min(ROCK_TOP + (across < 0 ? -across * 0.3 : across * 0.62), 0.92 - 0.11 * Math.max(0, 1 - Math.abs(across - 0.44) / 0.17)) + rough;
  if (v > crag) return mix(ROCK, ROCK_LIT, v < crag + 0.03 && glint > 0.55 ? 0.7 : glint * 0.25);

  for (const [at, foot, tall, wide] of [[0, 0.66, 0.13, 0.56], [1, 0.64, 0.15, 0.5], [0.64, 0.585, 0.032, 0.07], [0.31, 0.6, 0.026, 0.06]] as const) {
    const top = hill(u, at, foot, tall, wide), under = (v - top) / 0.06;
    if (under > 0 && under < 1 && top < foot - 0.012) return mix(RIDGE, FOG, under ** 1.5);
  }

  // A broad dome on the left and a table rock on the right, before a taller, fainter mountain.
  const fogLine = 0.47, sunk = (top: number) => Math.max(0, (v - top) / (fogLine - top)) ** 3;
  const near = 0.45 - Math.max(0.11 * Math.max(0, Math.cos((u - 0.2) / 0.25 * Math.PI / 2)) * (Math.abs(u - 0.2) < 0.25 ? 1 : 0), 0.075 * Math.min(1, Math.max(0, 2.4 * (1 - Math.abs(u - 0.83) / 0.075)))) - 0.004 * Math.sin(u * 97);
  const far = 0.45 - 0.2 * Math.max(0, 1 - Math.abs(u - 0.61) / 0.21) ** 1.3;
  if (v < fogLine && v > near) return mix(PEAK, FOG, sunk(near));
  if (v < fogLine && v > far) return mix(mix(PEAK, FOG_SKY, 0.5), FOG, sunk(far));
  if (v < fogLine) return mix(FOG_HIGH, FOG_SKY, Math.min(1, (v / 0.42) ** 1.4 + glint * 0.08 + 0.12 * Math.sin(v * 38 + u * 7)));

  return mix(FOG, FOG_HIGH, 0.14 + 0.1 * Math.sin(v * 46 + u * 5) + glint * 0.08);
}

/// The colour of the sunrise at a point (both from 0 to 1), with the sun `rise` of its way up.
function sunrise(u: number, v: number, aspect: number, rise: number, glint: number): Rgb {
  const sunV = HORIZON + 0.06 - rise * 0.3;
  const across = (u - SUN_X) * aspect, distance = Math.hypot(across, v - sunV);

  if (v < HORIZON) {
    const low = v / HORIZON;
    if (distance < 0.042) return SUN;
    const sky = mix(mix(SKY_HIGH, SKY_LOW, low), HAZE, low ** 3 * 0.55 * rise);
    return mix(sky, GLOW, Math.exp(-distance * distance / 0.035) * 0.7 * rise);
  }

  const near = (v - HORIZON) / (1 - HORIZON);
  const water = mix(mix(WATER_FAR, WATER_NEAR, near), HAZE, Math.exp(-across * across / 0.06) * 0.4 * rise * (1 - near));
  const lit = sunV - 0.042 < HORIZON && near < 0.85 && glint > 0.45 && Math.abs(across) < (0.012 + 0.05 * near) * (0.5 + glint);
  return lit ? mix(water, SUN, 0.35 + 0.65 * rise) : water;
}

/// The colour of the haystacks at a point (both from 0 to 1): two stacks dark against the
/// afterglow, their shadows long on the field, under a dusk sky over far hills.
function haystacks(u: number, v: number, aspect: number, glint: number): Rgb {
  // The sun has set behind the stacks: the sky still burns there and their rims catch it.
  const glow = Math.exp(-(((u - 0.57) * aspect) ** 2 / 0.5 + (v - 0.36) ** 2 / 0.02));

  for (const [at, foot, wide, tall] of STACKS) {
    // Walls that lean out as they rise, under a cone of thatch with its eaves past them.
    const across = (u - at) * aspect / wide, up = (foot - v) / tall;
    const edge = up < 0.5 ? 0.86 + 0.28 * up : 1.08 * (1 - (up - 0.5) / 0.5) ** 0.8;
    if (up > 0 && up < 1 && Math.abs(across) < edge) return mix(up < 0.5 ? STACK : THATCH, STACK_LIT, Math.max(0, Math.abs(across) / edge - 0.72) * 2.6 * (0.5 + 0.5 * glint) + glint * 0.1);
  }

  const hills = FIELD - 0.065 - 0.022 * Math.sin(u * 5.3 + 1) - 0.01 * Math.sin(u * 17);
  if (v < hills) {
    const sky = mix(mix(DUSK_HIGH, DUSK_LOW, (v / hills) ** 1.6), DUSK_HIGH, 0.22 * Math.max(0, Math.sin(v * 34 + u * 5)));
    return mix(sky, AFTERGLOW, glow * 0.85);
  }
  if (v < FIELD) return v > FIELD - 0.022 - 0.012 * Math.sin(u * 29) - 0.008 * Math.sin(u * 67) ? HEDGE : mix(mix(HILLS, DUSK_LOW, 0.2 + glint * 0.1), AFTERGLOW, glow * 0.45);

  const near = (v - FIELD) / (1 - FIELD), field = mix(mix(FIELD_FAR, FIELD_NEAR, near ** 0.7), AFTERGLOW, glint > 0.7 ? 0.22 * (1 - near) : 0);
  for (const [at, foot, wide, tall] of STACKS) {
    // The shadow falls toward us and to the left, longest for the taller stack.
    const down = (v - foot) / tall, across = ((u - at) * aspect + down * tall * 0.9) / wide;
    if (down > -0.12 && down < 0.5 && Math.abs(across) < 0.95 - down * 0.5) return mix(STACK_SHADOW, field, 0.15 + down);
  }

  return field;
}

/// The lily pads: across the width, down the pond (both from 0 to 1) and a lot that puts some
/// in flower. They lie in drifts, each a place and a number of pads.
const PADS = [[0.22, 0.52, 6], [0.84, 0.5, 7], [0.6, 0.63, 9], [0.3, 0.78, 9], [0.86, 0.85, 8], [0.5, 0.93, 6]].flatMap(([u, v, count], drift) => {
  const next = sequence(23 + drift);
  return Array.from({ length: count }, () => ({ u: u + (next() - 0.5) * 0.26, v: v + (next() - 0.5) * 0.09, bloom: next() }));
});

/// The colour of the lily pond at a point (both from 0 to 1): trees and willow behind the
/// bridge, and below them the water with the trees standing in it and drifts of pads on top.
function lilies(u: number, v: number, aspect: number, glint: number): Rgb {
  if (v < POND) {
    const clump = Math.sin(u * 23 + v * 9) + Math.sin(u * 61 - v * 17) + glint, leaves = mix(LEAVES, LEAVES_LIT, Math.max(0, Math.min(1, clump * 0.35)));
    return Math.sin(u * 140) > 0.7 && glint > 0.4 ? mix(leaves, WILLOW, 0.6) : leaves;
  }

  // Pads grow as they come nearer, and the far ones sink into the colour of the water.
  const near = (v - POND) / (1 - POND), size = 0.028 + 0.062 * near;
  for (const pad of PADS) {
    if (((u - pad.u) * aspect / size) ** 2 + ((v - pad.v) / (size * 0.32)) ** 2 < 1) return mix(mix(PAD_SHADE, PAD, glint), POND_GREEN, 0.3 * (1 - near));
  }

  const trees = 0.5 + 0.5 * Math.sin(u * 47 + near * 5) * glint, sky = Math.max(0, Math.sin(u * 7 + 0.6) * 0.6 + 0.4 * Math.sin(u * 19 + near * 3));
  return mix(mix(POND_DARK, POND_GREEN, trees), POND_SKY, sky * (0.25 + 0.4 * near));
}

/// The top of Parliament's skyline at `u`: its towers over the long roof with its pinnacles,
/// which runs down into the fog at its left end.
function skyline(u: number, aspect: number): number {
  let top = (Math.sin(u * 150) > 0.6 ? 0.4 : 0.425) + Math.max(0, 0.56 - u) * 2.4;
  for (const [at, wide, tip, fall] of SKYLINE) {
    const across = Math.abs(u - at) * aspect / wide;
    if (across < 1) top = Math.min(top, fall ? tip + fall * across : across > 0.78 ? tip - 0.035 : tip);
  }

  return top;
}

/// The colour of Parliament at a point (both from 0 to 1): towers in the fog against an evening
/// sky, the low sun beside them, and the river that carries both.
function parliament(u: number, v: number, aspect: number, glint: number): Rgb {
  const across = (u - LOW_SUN_X) * aspect;
  if (v < THAMES) {
    const top = skyline(u, aspect), distance = Math.hypot(across, v - LOW_SUN_V);
    // The fog is thickest at the water, where the walls fade into the sky's colour.
    if (v > top) return mix(TOWERS, EMBER, 0.1 + 0.32 * ((v - top) / (THAMES - top)) ** 2 + glint * 0.06);
    if (distance < 0.034) return LOW_SUN;

    const sky = mix(mix(EVENING, EMBER, (v / THAMES) ** 1.3), EVENING, 0.2 * Math.max(0, Math.sin(v * 30 + u * 4)));
    return mix(sky, GOLD, Math.exp(-distance * distance / 0.022) * 0.75);
  }

  const near = (v - THAMES) / (1 - THAMES), water = mix(mix(THAMES_FAR, THAMES_NEAR, near ** 0.7), GOLD, Math.exp(-across * across / 0.05) * 0.3 * (1 - near));
  if (near < 0.85 && glint > 0.45 && Math.abs(across) < (0.012 + 0.05 * near) * (0.5 + glint)) return mix(water, LOW_SUN, 0.85);

  return 2 * THAMES - v > skyline(u, aspect) ? mix(water, TOWERS, 0.6 * (1 - near)) : water;
}

function draw(target: HTMLCanvasElement, strokes: Stroke[], done: number, scene: Scene) {
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

  // The sun slows as it climbs and the light comes up with it; the haze is lit evenly.
  const rise = 1 - (1 - done) ** 3, light = scene === "sunrise" ? 0.5 + 0.5 * rise : 0.95, aspect = width / height;
  const colourAt = (u: number, v: number, glint: number) => {
    switch (scene) {
      case "sunrise": return sunrise(u, v, aspect, rise, glint);
      case "haze": return haze(u, v, glint);
      case "wanderer": return wanderer(u, v, aspect, glint);
      case "haystacks": return haystacks(u, v, aspect, glint);
      case "lilies": return lilies(u, v, aspect, glint);
      case "parliament": return parliament(u, v, aspect, glint);
    }
  };
  const stroke = (x: number, y: number, length: number, thick: number, tilt: number, colour: Rgb, shade: number) => {
    const dx = Math.cos(tilt) * length / 2, dy = Math.sin(tilt) * length / 2;
    context.strokeStyle = `rgb(${Math.round(colour[0] * shade)},${Math.round(colour[1] * shade)},${Math.round(colour[2] * shade)})`;
    context.lineWidth = thick;
    context.beginPath();
    context.moveTo(x - dx, y - dy);
    context.lineTo(x + dx, y + dy);
    context.stroke();
  };

  // An underpainting, so no gap between strokes shows the page.
  for (let y = 0; y < height; y += 6) {
    const colour = colourAt(0.05, y / height, 0);
    context.fillStyle = `rgb(${Math.round(colour[0] * light * 0.8)},${Math.round(colour[1] * light * 0.8)},${Math.round(colour[2] * light * 0.8)})`;
    context.fillRect(0, y, width, 6);
  }

  // The sunrise is repainted whole as the sun moves; the others fill in stroke by stroke.
  const shown = scene === "sunrise" ? strokes.length : Math.round(strokes.length * done);
  for (let i = 0; i < shown; i++) {
    const s = strokes[i];
    stroke(s.x, s.y, s.length, s.width, s.tilt, colourAt(s.x / width, s.y / height, s.glint), light * (0.86 + s.tint * 0.28));
  }

  if (scene === "haze" || scene === "haystacks") return;

  if (scene === "parliament") {
    // The towers once more in upright strokes, so their edges hold in the fog.
    context.globalAlpha = 0.6;
    for (let x = width * 0.5; x < width * (0.5 + 0.5 * done); x += 5) {
      const top = height * skyline(x / width, aspect), bank = height * THAMES;
      if (top < bank) stroke(x, (top + bank) / 2, bank - top, 6, Math.PI / 2, TOWERS, light);
    }
    context.globalAlpha = 1;
    return;
  }

  if (scene === "lilies") {
    // The footbridge, built from one bank to the other as the painting fills in: a deck and a
    // handrail in one arc, on posts. Then the flowers on their pads.
    const arc = (u: number) => height * (POND - 0.025 - 0.135 * (1 - ((u - 0.5) / BRIDGE_SPAN) ** 2)), step = BRIDGE_SPAN / 32;
    for (let i = 0; i < 64 * done; i++) {
      const u = 0.5 - BRIDGE_SPAN + i * step, x = width * u, tilt = Math.atan2(arc(u + step) - arc(u), width * step);
      stroke(x, arc(u), width * step * 1.3, 7, tilt, BRIDGE_SHADE, light);
      stroke(x, arc(u) - height * 0.07, width * step * 1.3, 4, tilt, BRIDGE, light);
      if (i % 4 === 0) stroke(x, arc(u) - height * 0.035, height * 0.07, 3, Math.PI / 2, BRIDGE, light);
    }
    for (const pad of PADS) {
      const size = 8 + 14 * (pad.v - POND) / (1 - POND);
      if (pad.bloom > 1 - 0.45 * done) stroke(width * pad.u, height * pad.v - size * 0.3, size, size * 0.6, 0, pad.bloom > 0.82 ? BLOOM_PALE : BLOOM, light);
    }
    return;
  }

  if (scene === "wanderer") {
    // The wanderer, his back to us: one foot up on the rock, a long coat, wind in his hair
    // and a cane. Each shape is a list of points, across and up from his feet, in his height.
    const tall = height * 0.36, x = width * WANDERER_X, feet = height * ROCK_TOP + tall * 0.03;
    const shape = (colour: Rgb, points: readonly (readonly [number, number])[]) => {
      context.fillStyle = `rgb(${colour.map(part => Math.round(part * light)).join(",")})`;
      context.beginPath();
      for (const [across, up] of points) context.lineTo(x + across * tall, feet - up * tall);
      context.fill();
    };

    shape(ROCK, [[-0.36, -0.05], [-0.2, 0.012], [-0.04, 0.012], [0.01, 0.07], [0.16, 0.07], [0.19, 0.035], [0.25, 0.03], [0.33, -0.06], [0.42, -0.22], [-0.5, -0.22]]);
    shape(LEGS, [[-0.07, 0.5], [-0.115, 0.03], [-0.15, 0], [-0.055, 0], [-0.045, 0.03], [-0.005, 0.5]]);
    shape(LEGS, [[0.005, 0.5], [0.045, 0.3], [0.04, 0.1], [0.025, 0.06], [0.14, 0.06], [0.105, 0.1], [0.11, 0.31], [0.08, 0.5]]);
    shape(COAT, [[-0.04, 0.865], [-0.115, 0.83], [-0.125, 0.7], [-0.085, 0.61], [-0.125, 0.4], [-0.03, 0.385], [0, 0.46], [0.03, 0.385], [0.125, 0.41], [0.085, 0.61], [0.115, 0.7], [0.115, 0.83], [0.04, 0.865]]);
    shape(COAT_LIT, [[0.04, 0.865], [0.115, 0.83], [0.115, 0.7], [0.085, 0.61], [0.125, 0.41], [0.085, 0.4], [0.055, 0.61], [0.08, 0.72], [0.07, 0.82]]);
    shape(COAT, [[-0.115, 0.83], [-0.15, 0.67], [-0.15, 0.51], [-0.115, 0.5], [-0.105, 0.67]]);
    shape(COAT, [[0.115, 0.83], [0.165, 0.68], [0.17, 0.51], [0.135, 0.5], [0.125, 0.66], [0.09, 0.73]]);
    shape(SKIN, [[-0.15, 0.51], [-0.15, 0.475], [-0.118, 0.47], [-0.115, 0.5]]);
    shape(SKIN, [[0.135, 0.5], [0.137, 0.465], [0.172, 0.47], [0.17, 0.51]]);
    shape(CANE, [[0.148, 0.5], [0.164, 0.5], [0.215, 0.03], [0.203, 0.03]]);
    shape(SKIN, [[-0.028, 0.905], [-0.028, 0.855], [0.028, 0.855], [0.028, 0.905]]);
    shape(HAIR, [[-0.052, 0.93], [-0.05, 0.985], [-0.01, 1.005], [0.045, 0.995], [0.08, 0.965], [0.052, 0.955], [0.05, 0.9], [-0.042, 0.89]]);
    return;
  }

  // Masts in the haze along the horizon, then two boats and their shadows on the water.
  const horizon = height * HORIZON;
  context.globalAlpha = 0.5;
  for (const [at, tall] of [[0.1, 0.16], [0.16, 0.1], [0.23, 0.13], [0.77, 0.09], [0.84, 0.15], [0.9, 0.11]] as const) {
    stroke(width * at, horizon - height * tall / 2, height * tall, 3, Math.PI / 2 + 0.04, MAST, light);
    stroke(width * at, horizon + 2, 30, 6, 0, MAST, light);
  }
  context.globalAlpha = 1;
  for (const [at, down, size] of [[0.44, 0.52, 1], [0.35, 0.24, 0.6]] as const) {
    const x = width * at, y = horizon + (height - horizon) * down;
    stroke(x, y, 54 * size, 8 * size, -0.03, BOAT, light);
    stroke(x - 6 * size, y - 12 * size, 18 * size, 6 * size, Math.PI / 2, BOAT, light);
    for (let i = 1; i <= 3; i++) stroke(x + i * 3, y + i * 9 * size, (40 - i * 9) * size, 4 * size, 0, BOAT, light * 1.25);
  }
}

/// A finished painting kept to move it: its own pixels, the ones on the canvas, each pixel's light
/// (128 is its own) and how far each row sways.
interface Life { base: Uint8ClampedArray; live: ImageData; glow: Uint8Array; reach: Float32Array }

function hold(target: HTMLCanvasElement, scene: Scene): Life | null {
  const context = target.getContext("2d"), width = target.width, height = target.height;
  if (!context || !width || !height) return null;

  const base = context.getImageData(0, 0, width, height).data;
  // Waves widen toward the viewer from where the water or the ground begins. The wanderer stands
  // in his fog, so it only sways as the rest does.
  let shore = height;
  if (scene !== "wanderer") for (let y = height - 1; y >= 0 && level(scene, y / height); y--) shore = y;

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
