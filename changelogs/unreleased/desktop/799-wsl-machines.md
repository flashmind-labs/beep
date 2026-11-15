### Added

- Desktop: a Windows machine lists its WSL distributions with the coding
  agents installed in each. Opening its page or the Add a machine sheet
  also looks into stopped distributions. Add a distribution as a machine
  with one button. Beep installs beep-node inside it and keeps it running,
  so agents installed in WSL can run there.
- Desktop: a WSL machine can be turned off and on again from its Windows
  machine's page or the Add a machine sheet. A turned-off one shows "Off"
  with "Turn on".
- Desktop: when a project's only checkout is on a machine where no coding
  agent can run yet, such as when the agent's adapter failed to install,
  the project bar names that machine and says why. A button opens it.
