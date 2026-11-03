// When the Cmd+K palette asks for hits. Each keystroke asks at once for
// what the app holds (`full` false), which draws on that keystroke, then
// once typing pauses for the full search, which also reads chat messages
// and may ask the server. An answer only draws while its query is still
// the one typed, and never over an answer to a later ask.

export const DEBOUNCE_MS = 120;

export type Finder<H> = (query: string, full: boolean) => Promise<H[]>;
export type Drawer<H> = (query: string, hits: H[]) => void;

export class PaletteSearch<H> {
  private query = "";
  /// Bumped on every ask; an answer carries the number it was sent with.
  private asked = 0;
  /// The number of the ask whose answer is on screen.
  private drawn = 0;
  /// Bumped when the query changes, so answers to the old one never draw.
  private typed = 0;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private stopped = false;

  private find: Finder<H>;
  private draw: Drawer<H>;

  constructor(find: Finder<H>, draw: Drawer<H>) {
    this.find = find;
    this.draw = draw;
  }

  /// The query changed.
  type(query: string) {
    this.query = query;
    this.typed++;
    this.ask(false);
    this.later();
  }

  /// What the app holds changed: ask again for the same query.
  refresh() {
    this.ask(false);
    this.later();
  }

  /// The palette closed: nothing more draws.
  stop() {
    this.stopped = true;
    clearTimeout(this.timer);
  }

  private later() {
    clearTimeout(this.timer);
    this.timer = setTimeout(() => this.ask(true), DEBOUNCE_MS);
  }

  private async ask(full: boolean) {
    if (this.stopped) return;
    const mine = ++this.asked, typed = this.typed, query = this.query;
    let hits: H[];
    try {
      hits = await this.find(query, full);
    } catch (e) {
      console.warn("searching:", e);
      return;
    }
    if (this.stopped || typed !== this.typed || mine < this.drawn) return;
    this.drawn = mine;
    this.draw(query, hits);
  }
}
