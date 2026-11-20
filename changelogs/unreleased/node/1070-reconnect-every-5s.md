### Changed

- beep-node: a node tries to reach another machine, and syncs files with
  it, again every 5 seconds instead of waiting up to a minute. Each try
  gives up after 10 seconds.
