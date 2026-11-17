### Added

- Desktop: Cmd+K finds sessions by what the agent replied, not only by
  your prompts. A copy of each machine's index updates live, so results
  come at once, and sessions on offline machines are still found.

### Changed

- Desktop: after updating, each machine indexes its sessions once in the
  background. A prompt you just sent shows in search once its session has
  been quiet for a minute or two.

### Performance

- Desktop: the app no longer reads every transcript over the mesh to build
  its own index, which saves data and battery.
