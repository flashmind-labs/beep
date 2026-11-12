### Performance

- Desktop: a GitHub or Forgejo host that a machine cannot reach is no longer
  asked again for every issue and pull request list. Beep waits a minute
  before trying it again on that machine, then longer up to 15 minutes, so
  startup no longer spends minutes of CPU on failed host commands.
