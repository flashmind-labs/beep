### Fixed

- Desktop: fixing a failed workflow run that a tag started, such as a
  release, now starts the agent on a new branch from that tag. Before,
  it failed because no branch had the tag's name.
