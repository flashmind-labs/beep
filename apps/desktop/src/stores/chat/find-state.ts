interface TextMatch { start: number; end: number }

/** Compile once per query; invalid regular expressions retain their SyntaxError. */
export function searchPattern(query: string, regex = false): RegExp | null {
  if (!query) return null;
  return new RegExp(regex ? query : query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "gimu");
}

/** Ignore empty matches, which have no text to highlight or navigate to. */
export function textMatches(text: string, query: string | RegExp | null): TextMatch[] {
  const pattern = typeof query === "string" ? searchPattern(query) : query;
  if (!pattern) return [];
  return [...text.matchAll(pattern)].filter(match => match[0].length)
    .map(match => ({ start: match.index, end: match.index + match[0].length }));
}

export function nextMatch(current: number, count: number, step: number) {
  return count ? ((current + step) % count + count) % count : -1;
}
