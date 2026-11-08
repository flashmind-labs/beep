// The pages a window went through, for ⌘[ and ⌘].

/// Pages kept to go back to.
const MAX_PAGES = 100;

export class PageHistory<T> {
  private past: T[] = [];
  private ahead: T[] = [];

  /// Leaves `page` for a new one. Going to the page it already was keeps
  /// no step, and ends what was ahead only when the page changes.
  visit(page: T, next: T | null = null, replace = false) {
    if (next !== null && JSON.stringify(page) === JSON.stringify(next)) return;
    if (!replace && JSON.stringify(this.past.at(-1)) !== JSON.stringify(page)) this.past.push(page);
    if (this.past.length > MAX_PAGES) this.past.shift();
    this.ahead.length = 0;
  }

  /// The page before `current`, which becomes the one ahead.
  back(current: T): T | null {
    const page = this.past.pop() ?? null;
    if (page !== null) this.ahead.push(current);
    return page;
  }

  /// The page after `current`, which becomes the one behind.
  forward(current: T): T | null {
    const page = this.ahead.pop() ?? null;
    if (page !== null) this.past.push(current);
    return page;
  }
}
