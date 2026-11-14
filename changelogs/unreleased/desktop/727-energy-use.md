### Performance

- Desktop: while a project syncs, its sync details are read at most
  every ten seconds instead of on every change, which uses less battery.
- Desktop: the app log no longer fills with network warnings after
  waking from sleep, and writes one line per request to another machine
  instead of three.
