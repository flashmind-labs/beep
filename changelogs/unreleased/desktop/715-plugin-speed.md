### Changed

- Desktop: plugins cost nothing while idle. Animations are played by the page from
  a declaration, so the example cat no longer wakes its plugin for every blink.
- Desktop: plugin images are sent to the page once instead of in every update.
- Desktop: a plugin can start only when the slot it draws in appears, and one that
  draws only in slots stops a minute after they leave the page.
- Desktop: plugins pause while no window is visible, and animations stop and show
  their resting frame while the window is hidden or reduced motion is on.

### Fixed

- Desktop: a plugin that uses more than half a second of script time in a minute is
  stopped with a reason instead of slowing the app.
