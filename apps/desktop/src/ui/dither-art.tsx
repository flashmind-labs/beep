// A sunset over water drawn in dots: a computed scene put through ordered
// dithering, in the skin's own accent. It is still: drawn once, and again
// only when its box or the skin changes.
import { useLayoutEffect, useRef } from "react";

export function DitherArt() {
  const canvas = useRef<HTMLCanvasElement>(null);
  useLayoutEffect(() => {
    const target = canvas.current;
    if (!target) return;

    const paint = () => draw(target);
    const resized = new ResizeObserver(paint), skinned = new MutationObserver(paint);
    resized.observe(target);
    skinned.observe(document.documentElement, { attributes: true, attributeFilter: ["data-skin"] });
    return () => { resized.disconnect(); skinned.disconnect(); };
  }, []);

  return <canvas ref={canvas} className="dither-art" aria-hidden="true" />;
}

/// A dot's pitch in CSS pixels, and the share of it the dot fills.
const PITCH = 3, DOT = 2;
/// How many colours the scene's warmth is drawn in.
const TONES = 8;
/// Where the water meets the sky, as a share of the height.
const HORIZON = 0.64;
/// The 8 by 8 Bayer matrix: the threshold each dot's light is held against.
const BAYER = [
  0, 32, 8, 40, 2, 34, 10, 42, 48, 16, 56, 24, 50, 18, 58, 26,
  12, 44, 4, 36, 14, 46, 6, 38, 60, 28, 52, 20, 62, 30, 54, 22,
  3, 35, 11, 43, 1, 33, 9, 41, 51, 19, 59, 27, 49, 17, 57, 25,
  15, 47, 7, 39, 13, 45, 5, 37, 63, 31, 55, 23, 61, 29, 53, 21,
];

function draw(target: HTMLCanvasElement) {
  const context = target.getContext("2d");
  const width = target.clientWidth, height = target.clientHeight;
  if (!context || !width || !height) return;

  const scale = window.devicePixelRatio || 1;
  target.width = Math.round(width * scale);
  target.height = Math.round(height * scale);
  context.setTransform(scale, 0, 0, scale, 0, 0);
  context.clearRect(0, 0, width, height);

  const tones = palette(target, context);
  const columns = Math.ceil(width / PITCH), rows = Math.ceil(height / PITCH), aspect = width / height;

  for (let row = 0; row < rows; row++) {
    for (let column = 0; column < columns; column++) {
      const u = (column + 0.5) / columns, v = (row + 0.5) / rows;
      const { light, warmth } = scene(u, v, aspect);
      const threshold = (BAYER[(row & 7) * 8 + (column & 7)] + 0.5) / 64;
      if (light * edgeFade(u, v) <= threshold) continue;

      context.fillStyle = tones[Math.min(TONES - 1, Math.floor(warmth * TONES))];
      context.fillRect(column * PITCH, row * PITCH, DOT, DOT);
    }
  }
}

/// The scene's light and warmth at a point, both from 0 to 1: a low sun with
/// its glow, bands of cloud, and the sun's column on rippled water.
function scene(u: number, v: number, aspect: number): { light: number; warmth: number } {
  const across = (u - 0.5) * aspect;
  if (v < HORIZON) {
    const rise = v / HORIZON;
    const sun = Math.hypot(across, v - (HORIZON - 0.09));
    const glow = Math.exp(-sun * sun / 0.03);
    const band = ramp(0.48, 0.72, clouds(u * 1.9 * aspect, v * 9));
    const light = 0.06 + 0.4 * rise * rise + 0.55 * glow + 0.34 * band * (1 - rise * 0.4) + (sun < 0.075 ? 1 : 0);
    return { light: Math.min(1, light), warmth: Math.min(1, rise * rise * 0.9 + glow) };
  }

  const depth = (v - HORIZON) / (1 - HORIZON);
  const column = Math.exp(-(across * across) / (0.004 + depth * 0.05));
  const ripple = 0.5 + 0.5 * Math.sin(v * 260 + Math.sin(u * 40) * 1.5);
  const light = (0.3 * (1 - depth) + 0.75 * column) * (0.45 + 0.55 * ripple);
  return { light: Math.min(1, light), warmth: Math.min(1, (1 - depth) * 0.8 + column * 0.6) };
}

/// The picture thins out toward its edges, so it has no frame.
const edgeFade = (u: number, v: number) => ramp(0, 0.28, u) * ramp(1, 0.72, u) * ramp(0, 0.12, v) * ramp(0.72, 0.58, v);
const ramp = (from: number, to: number, value: number) => {
  const t = Math.min(1, Math.max(0, (value - from) / (to - from)));
  return t * t * (3 - 2 * t);
};

const clouds = (x: number, y: number) => noise(x, y) * 0.5 + noise(x * 2.1, y * 2.1) * 0.3 + noise(x * 4.3, y * 4.3) * 0.2;
function noise(x: number, y: number) {
  const left = Math.floor(x), top = Math.floor(y), tx = ramp(0, 1, x - left), ty = ramp(0, 1, y - top);
  const a = grain(left, top), b = grain(left + 1, top), c = grain(left, top + 1), d = grain(left + 1, top + 1);
  return a + (b - a) * tx + (c - a) * ty + (a - b - c + d) * tx * ty;
}
const grain = (x: number, y: number) => {
  const wave = Math.sin(x * 127.1 + y * 311.7) * 43758.5453;
  return wave - Math.floor(wave);
};

/// The colours from the cool sky to the warm horizon and the sun's white: the
/// skin's accent, then its hot colour, then its text colour.
function palette(target: HTMLCanvasElement, context: CanvasRenderingContext2D): string[] {
  const skin = getComputedStyle(target);
  const read = (name: string, fallback: Rgb): Rgb => {
    context.fillStyle = "#000";
    context.fillStyle = skin.getPropertyValue(name).trim();
    const hex = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(String(context.fillStyle));
    return hex && hex[0] !== "#000000" ? [parseInt(hex[1], 16), parseInt(hex[2], 16), parseInt(hex[3], 16)] : fallback;
  };
  const cool = read("--accent", [94, 106, 210]), warm = read("--hot", [242, 153, 74]), white = read("--fg", [227, 228, 230]);

  return Array.from({ length: TONES }, (_, index) => {
    const t = index / (TONES - 1);
    const [from, to, share] = t < 0.7 ? [cool, warm, t / 0.7] : [warm, white, (t - 0.7) / 0.3];
    return `rgb(${from.map((channel, at) => Math.round(channel + (to[at] - channel) * share)).join(" ")})`;
  });
}
type Rgb = [number, number, number];
