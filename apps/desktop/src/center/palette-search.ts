// What the Cmd+K palette draws when. What the window holds draws at once;
// chat text is searched by the app, a moment after typing stops.

export const DEBOUNCE_MS = 120;
/// Chat text is searched from this many characters.
const MIN_TEXT_QUERY = 2;

export type TextFinder<T> = (query: string) => Promise<T[]>;
export type Drawer<T> = (query: string, text: T[]) => void;

export class PaletteSearch<T> {
  private query = "";
  /// The chats holding the query in their text, as far as known.
  private text: T[] = [];
  private timer: ReturnType<typeof setTimeout> | undefined;
  /// One search of chat text at a time: it walks every message.
  private asking = false;
  private stopped = false;

  private findText: TextFinder<T>;
  private draw: Drawer<T>;

  constructor(findText: TextFinder<T>, draw: Drawer<T>) {
    this.findText = findText;
    this.draw = draw;
  }

  /// The query changed.
  type(query: string) {
    this.query = query;
    this.text = [];
    this.draw(query, this.text);
    clearTimeout(this.timer);
    this.timer = undefined;
    if (query.trim().length >= MIN_TEXT_QUERY) {
      this.timer = setTimeout(() => { this.timer = undefined; this.ask(); }, DEBOUNCE_MS);
    }
  }

  /// What the window holds changed: draw the same query again.
  refresh() {
    if (!this.stopped) this.draw(this.query, this.text);
  }

  /// The palette closed: nothing more draws.
  stop() {
    this.stopped = true;
    clearTimeout(this.timer);
  }

  private async ask() {
    if (this.stopped || this.asking) return;
    const query = this.query;
    this.asking = true;
    let text: T[] | null = null;
    try {
      text = await this.findText(query);
    } catch (e) {
      console.warn("searching chat text:", e);
    }
    this.asking = false;
    if (this.stopped) return;
    if (query === this.query) {
      if (text) { this.text = text; this.draw(query, text); }
      // Typed while this one ran, and its wait is over: search what it says now.
    } else if (this.timer === undefined && this.query.trim().length >= MIN_TEXT_QUERY) {
      this.ask();
    }
  }
}
