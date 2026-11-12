### Performance

- Node: background session copying reads its cache once per pass, stops
  trying sessions once the cache is full, and skips passes while nothing
  changed, instead of rescanning the cache for every session it skips.
