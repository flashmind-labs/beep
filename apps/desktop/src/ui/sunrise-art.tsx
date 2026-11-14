// A sunrise over a harbour painted in short brush strokes, after Monet's
// "Impression, Sunrise". The sun rises once, over RISE_MS, and the painting
// is still from then on; with Reduce motion on it is drawn risen.
import { useLayoutEffect, useRef } from "react";

export function SunriseArt() {
  const canvas = useRef<HTMLCanvasElement>(null);
  useLayoutEffect(() => {
    const target = canvas.current;
    if (!target) return;

    let frame = 0, strokes: Stroke[] = [], began: number | null = null, last = 0;
    const still = typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const paint = (now: number) => {
      began ??= now;
      const risen = still ? 1 : Math.min(1, (now - began) / RISE_MS);

      if (risen === 1 || now - last >= FRAME_MS) {
        last = now;
        draw(target, strokes, risen);
      }
      frame = risen < 1 ? requestAnimationFrame(paint) : 0;
    };
    const resized = new ResizeObserver(() => {
      strokes = layStrokes(target.clientWidth, target.clientHeight);
      if (!frame) frame = requestAnimationFrame(paint);
    });
    resized.observe(target);
    return () => { resized.disconnect(); if (frame) cancelAnimationFrame(frame); };
  }, []);

  return <canvas ref={canvas} className="dither-art sunrise-art" aria-hidden="true" />;
}

/// How long the sun takes to rise, and the time between two painted frames.
const RISE_MS = 9000, FRAME_MS = 33;
/// The side of one painted pixel in CSS pixels: 1 paints smoothly, more paints in blocks.
const PIXEL = 4;
/// Where the water meets the sky, and where the sun stands across the width.
const HORIZON = 0.58, SUN_X = 0.57;

type Rgb = readonly [number, number, number];
/// One brush stroke: its middle, size and tilt, and the chance values that
/// shift its colour (`tint`) and decide whether it catches the sun (`glint`).
interface Stroke { x: number; y: number; length: number; width: number; tilt: number; tint: number; glint: number }

const SKY_HIGH: Rgb = [50, 64, 92], SKY_LOW: Rgb = [124, 120, 134], HAZE: Rgb = [186, 124, 110], GLOW: Rgb = [222, 132, 92];
const SUN: Rgb = [244, 100, 46], WATER_FAR: Rgb = [88, 102, 120], WATER_NEAR: Rgb = [38, 56, 78], BOAT: Rgb = [20, 32, 46], MAST: Rgb = [66, 80, 102];

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
function layStrokes(width: number, height: number): Stroke[] {
  const next = sequence(7), strokes: Stroke[] = [];
  const add = (x: number, y: number, length: number, thick: number) => {
    const water = y / height > HORIZON;
    strokes.push({ x, y, length: length * (0.7 + next() * 0.6), width: thick * (0.7 + next() * 0.6), tilt: (next() - 0.5) * (water ? 0.12 : 0.5), tint: next(), glint: next() });
  };

  for (let y = 0; y < height + 10; y += 10) {
    for (let x = 0; x < width + 26; x += 26) add(x + (next() - 0.5) * 20, y + (next() - 0.5) * 8, 40, 13);
  }
  for (let i = Math.round(width * height / 260); i > 0; i--) add(next() * width, next() * height, 18, 5);

  return strokes;
}

/// The colour of the scene at a point (both from 0 to 1), with the sun `rise` of its way up.
function scene(u: number, v: number, aspect: number, rise: number, glint: number): Rgb {
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

function draw(target: HTMLCanvasElement, strokes: Stroke[], risen: number) {
  const context = target.getContext("2d");
  const width = target.clientWidth, height = target.clientHeight;
  if (!context || !width || !height) return;

  const scale = PIXEL > 1 ? 1 / PIXEL : Math.min(window.devicePixelRatio || 1, 2);
  if (target.width !== Math.round(width * scale) || target.height !== Math.round(height * scale)) {
    target.width = Math.round(width * scale);
    target.height = Math.round(height * scale);
  }
  context.setTransform(scale, 0, 0, scale, 0, 0);
  context.lineCap = "round";

  // Slows as it climbs; the light comes up with it.
  const rise = 1 - (1 - risen) ** 3, light = 0.5 + 0.5 * rise, aspect = width / height;
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
    const colour = scene(0.05, y / height, aspect, rise, 0);
    context.fillStyle = `rgb(${Math.round(colour[0] * light)},${Math.round(colour[1] * light)},${Math.round(colour[2] * light)})`;
    context.fillRect(0, y, width, 6);
  }

  for (const s of strokes) stroke(s.x, s.y, s.length, s.width, s.tilt, scene(s.x / width, s.y / height, aspect, rise, s.glint), light * (0.86 + s.tint * 0.28));

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
