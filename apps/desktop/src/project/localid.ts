// The id a project has with no server to make one: a version 5 UUID of its
// name, as the desktop's mesh computes it, so Actions and pull requests are
// asked for by the same id.

const URL_NAMESPACE = "6ba7b811-9dad-11d1-80b4-00c04fd430c8";

function sha1(bytes: Uint8Array) {
  const padded = new Uint8Array(((bytes.length + 8) >> 6 << 6) + 64);
  padded.set(bytes);
  padded[bytes.length] = 0x80;
  const view = new DataView(padded.buffer);
  view.setUint32(padded.length - 4, bytes.length * 8 >>> 0);
  view.setUint32(padded.length - 8, Math.floor(bytes.length / 0x20000000));
  let [a, b, c, d, e] = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0];
  const w = new Uint32Array(80);
  const rol = (x: number, n: number) => (x << n) | (x >>> (32 - n));
  for (let at = 0; at < padded.length; at += 64) {
    for (let i = 0; i < 16; i++) w[i] = view.getUint32(at + i * 4);
    for (let i = 16; i < 80; i++) w[i] = rol(w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16], 1);
    let [x, y, z, u, v] = [a, b, c, d, e];
    for (let i = 0; i < 80; i++) {
      const [f, k] = i < 20 ? [(y & z) | (~y & u), 0x5a827999] : i < 40 ? [y ^ z ^ u, 0x6ed9eba1] : i < 60 ? [(y & z) | (y & u) | (z & u), 0x8f1bbcdc] : [y ^ z ^ u, 0xca62c1d6];
      const t = (rol(x, 5) + f + v + k + w[i]) >>> 0;
      [v, u, z, y, x] = [u, z, rol(y, 30) >>> 0, x, t];
    }
    [a, b, c, d, e] = [(a + x) >>> 0, (b + y) >>> 0, (c + z) >>> 0, (d + u) >>> 0, (e + v) >>> 0];
  }
  const out = new Uint8Array(20);
  const o = new DataView(out.buffer);
  [a, b, c, d, e].forEach((n, i) => o.setUint32(i * 4, n));
  return out;
}

const ids = new Map<string, string>();

/// The id of project `name` (`org/repo`).
export function localProjectId(name: string) {
  const kept = ids.get(name);
  if (kept) return kept;
  const ns = Uint8Array.from(URL_NAMESPACE.replace(/-/g, "").match(/../g)!, h => parseInt(h, 16));
  const text = new TextEncoder().encode(`beep:project:${name}`);
  const joined = new Uint8Array(ns.length + text.length);
  joined.set(ns);
  joined.set(text, ns.length);
  const sum = sha1(joined).slice(0, 16);
  sum[6] = (sum[6] & 0x0f) | 0x50;
  sum[8] = (sum[8] & 0x3f) | 0x80;
  const hex = [...sum].map(b => b.toString(16).padStart(2, "0")).join("");
  const id = `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
  ids.set(name, id);
  return id;
}
