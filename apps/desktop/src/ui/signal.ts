/// A change the owner of some state announces and the views of it follow.
export interface Signal<T = void> {
  follow(listener: (value: T) => void): void;
  notify(value: T): void;
}

export function signal<T = void>(): Signal<T> {
  const listeners = new Set<(value: T) => void>();

  return {
    follow: listener => { listeners.add(listener); },
    notify: value => { for (const listener of listeners) listener(value); },
  };
}
