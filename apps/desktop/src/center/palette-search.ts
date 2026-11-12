// Local lists draw immediately. The text index answers after typing stops.

export const DEBOUNCE_MS = 120;
const MIN_TEXT_QUERY = 2;

export type TextFinder<T> = (query: string) => Promise<T[]>;
export type Drawer<T> = (query: string, text: T[]) => void;

export class PaletteSearch<T> {
  private query = "";
  private version = 0;
  private find: TextFinder<T>;
  private text: T[] = [];
  private asking = false;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private stopped = false;
  private draw: Drawer<T>;

  constructor(find: TextFinder<T>, draw: Drawer<T>) {
    this.find = find;
    this.draw = draw;
  }

  type(query: string) {
    if (this.stopped) return;
    this.query = query;
    this.version++;
    this.text = [];
    this.refresh();
    clearTimeout(this.timer);
    this.timer = undefined;
    if (query.trim().length >= MIN_TEXT_QUERY) {
      this.timer = setTimeout(() => {
        this.timer = undefined;
        void this.ask();
      }, DEBOUNCE_MS);
    }
  }

  /// The lists changed; keep the text matches already found.
  refresh() {
    if (!this.stopped) this.draw(this.query, this.text);
  }

  /// A background index changed, even if the query did not.
  requery() {
    if (this.stopped || this.timer !== undefined || this.query.trim().length < MIN_TEXT_QUERY) return;
    // Coalesce indexing bursts without postponing the next search or
    // clearing matches already on screen.
    this.version++;
    this.timer = setTimeout(() => {
      this.timer = undefined;
      void this.ask();
    }, DEBOUNCE_MS);
  }

  stop() {
    this.stopped = true;
    clearTimeout(this.timer);
  }

  private async ask() {
    if (this.stopped || this.asking) return;
    const { query, version } = this;
    this.asking = true;
    let text: T[] | null = null;

    try {
      text = await this.find(query);
    } catch (e) {
      console.warn("searching palette text:", e);
    }

    this.asking = false;
    if (this.stopped) return;

    if (version === this.version) {
      if (text) { this.text = text; this.refresh(); }
    } else if (this.timer === undefined && this.query.trim().length >= MIN_TEXT_QUERY) {
      void this.ask();
    }
  }
}
