/// An image a plugin placed, named by its content: the host serves the bytes once per hash.
export interface AssetRef {
  /// 64 lowercase hex digits.
  hash: string;
  /// An `image/*` type for the blob.
  mime: string;
}

const HASH = /^[0-9a-f]{64}$/;
const MIME = /^image\/[A-Za-z0-9.+-]{1,64}$/;

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null && !Array.isArray(value);

/// Reads an asset reference from the host, or null when it is not one.
export function parseAssetRef(raw: unknown): AssetRef | null {
  if (!isRecord(raw)) return null;
  const { hash, mime } = raw;
  return typeof hash === "string" && HASH.test(hash) && typeof mime === "string" && MIME.test(mime) ? { hash, mime } : null;
}

/// What the cache needs from the browser and the desktop.
export interface AssetBackend {
  /// The bytes of the image with this hash.
  load(hash: string): Promise<ArrayBuffer>;
  /// A URL for the bytes. `revoke` releases it.
  create(bytes: ArrayBuffer, mime: string): string;
  revoke(url: string): void;
}

/// Blob URLs of the images placements use, fetched once per hash.
export interface AssetCache {
  /// Declares the images in use now. New ones are fetched, once each, and ones no longer
  /// listed have their blob URL revoked.
  use(refs: readonly AssetRef[]): void;
  /// A map from hash to blob URL for the images that have arrived.
  urls(): ReadonlyMap<string, string>;
}

type Entry = { state: "loading" } | { state: "ready"; url: string } | { state: "failed" };

/// `changed` runs each time an image arrives or is dropped. An image that fails to load is
/// not asked for again until it has been out of use.
export function createAssetCache(backend: AssetBackend, changed: () => void): AssetCache {
  const entries = new Map<string, Entry>();
  let urls: ReadonlyMap<string, string> = new Map();

  const publish = () => {
    const next = new Map<string, string>();
    for (const [hash, entry] of entries) if (entry.state === "ready") next.set(hash, entry.url);
    urls = next;
    changed();
  };

  const fetch = (ref: AssetRef) => {
    entries.set(ref.hash, { state: "loading" });
    backend.load(ref.hash).then(bytes => {
      // The image went out of use while it loaded.
      if (entries.get(ref.hash)?.state !== "loading") return;
      entries.set(ref.hash, { state: "ready", url: backend.create(bytes, ref.mime) });
      publish();
    }, () => {
      if (entries.get(ref.hash)?.state === "loading") entries.set(ref.hash, { state: "failed" });
    });
  };

  return {
    use(refs) {
      const wanted = new Map(refs.map(ref => [ref.hash, ref]));
      let dropped = false;
      for (const [hash, entry] of entries) {
        if (wanted.has(hash)) continue;
        if (entry.state === "ready") { backend.revoke(entry.url); dropped = true; }
        entries.delete(hash);
      }
      for (const [hash, ref] of wanted) if (!entries.has(hash)) fetch(ref);
      if (dropped) publish();
    },
    urls: () => urls,
  };
}

/// The browser's blob URLs, with bytes read through `invoke("plugins_asset")`.
export function browserBackend(load: (hash: string) => Promise<unknown>): AssetBackend {
  return {
    load: async hash => {
      const bytes = await load(hash);
      if (bytes instanceof ArrayBuffer) return bytes;
      throw new Error("the desktop sent no image bytes");
    },
    create: (bytes, mime) => URL.createObjectURL(new Blob([bytes], { type: mime })),
    revoke: url => URL.revokeObjectURL(url),
  };
}
