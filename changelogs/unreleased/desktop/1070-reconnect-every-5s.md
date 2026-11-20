### Changed

- Desktop: after a machine's connection drops, the app tries to reach it
  again every 5 seconds instead of waiting longer and longer between
  tries. Each try gives up after 10 seconds. Terminals, file tabs and
  session views keep trying while they are on screen instead of giving
  up after a few tries.
