### Performance

- Desktop: Beep opens about a third faster. The window shows before the rest
  of the app is set up, and the metrics clock no longer holds the launch for
  200 ms on Apple silicon.

### Changed

- Desktop: the log records how long each stage of a launch took, from the
  process starting to Home being drawn, in lines that start with `startup:`.
