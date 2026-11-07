// Local lists draw immediately. Independent text indexes answer after
// typing stops, so a slow source cannot hold back another source's hits.

export const DEBOUNCE_MS = 120;
const MIN_TEXT_QUERY = 2;

export type TextFinder<T> = (query: string) => Promise<T[]>;
export type Drawer<T> = (query: string, text: T[]) => void;

type Source<T> = { find: TextFinder<T>; text: T[]; asking: boolean };

export class PaletteSearch<T> {
  private query = "";
  private version = 0;
  private sources: Source<T>[];
  private timer: ReturnType<typeof setTimeout> | undefined;
  private stopped = false;
  private draw: Drawer<T>;

  constructor(finders: TextFinder<T> | TextFinder<T>[], draw: Drawer<T>) {
    this.sources = (Array.isArray(finders) ? finders : [finders])
      .map(find => ({ find, text: [], asking: false }));
    this.draw = draw;
  }

  type(query: string) {
    if (this.stopped) return;
    this.query = query;
    this.version++;
    for (const source of this.sources) source.text = [];
    this.refresh();
    clearTimeout(this.timer);
    this.timer = undefined;
    if (query.trim().length >= MIN_TEXT_QUERY) {
      this.timer = setTimeout(() => {
        this.timer = undefined;
        for (const source of this.sources) this.ask(source);
      }, DEBOUNCE_MS);
    }
  }

  /// The lists changed; keep the text matches already found.
  refresh() {
    if (!this.stopped) this.draw(this.query, this.sources.flatMap(source => source.text));
  }

  /// A background index changed, even if the query did not.
  requery() {
    if (this.stopped || this.timer !== undefined || this.query.trim().length < MIN_TEXT_QUERY) return;
    // Coalesce indexing bursts without postponing the next search or
    // clearing matches already on screen.
    this.version++;
    this.timer = setTimeout(() => {
      this.timer = undefined;
      for (const source of this.sources) this.ask(source);
    }, DEBOUNCE_MS);
  }

  stop() {
    this.stopped = true;
    clearTimeout(this.timer);
  }

  private async ask(source: Source<T>) {
    if (this.stopped || source.asking) return;
    const { query, version } = this;
    source.asking = true;
    let text: T[] | null = null;
    try {
      text = await source.find(query);
    } catch (e) {
      console.warn("searching palette text:", e);
    }
    source.asking = false;
    if (this.stopped) return;
    if (version === this.version) {
      if (text) { source.text = text; this.refresh(); }
    } else if (this.timer === undefined && this.query.trim().length >= MIN_TEXT_QUERY) {
      this.ask(source);
    }
  }
}
