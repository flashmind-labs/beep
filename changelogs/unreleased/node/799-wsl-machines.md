### Added

- Windows: the node runs the WSL machines it added only while Beep may use
  the computer. It leaves one alone when another Beep on the PC already
  runs it. If you stop WSL, for example with `wsl --shutdown`, the node
  waits until the machine is turned on again or the node restarts. A
  distribution removed from WSL is forgotten.
- WSL: a node running in WSL is named after its distribution and the
  Windows PC, such as Ubuntu on gpu-box.

### Fixed

- Linux (including WSL 1): installing Node.js for a coding agent's adapter,
  and the gh, teacli and flsh tools, no longer depends on the system `tar`.
  That `tar` fails on Ubuntu 24.04 under WSL 1, so these installs failed
  there.
