### Added

- Windows: the node runs the WSL machines it added only while Beep may use
  the computer. It leaves one alone when another Beep on the PC already
  runs it. If you stop WSL, for example with `wsl --shutdown`, the node
  waits until the machine is turned on again or the node restarts. A
  distribution removed from WSL is forgotten.
- WSL: a node running in WSL is named after its distribution and the
  Windows PC, such as Ubuntu on gpu-box.
