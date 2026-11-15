### Fixed

- Node: the first Git copy of a project into a folder that already has
  files no longer fails forever with "rebuilding the git index failed" when
  a file differs from the commit. The differing file stays as a local
  change.
