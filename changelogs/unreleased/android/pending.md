### Changed

- Android: diagrams in chats use the chat's colors instead of a white card, show at full size, and say what kind of diagram they are. Copy image, PNG and SVG moved into an Export menu next to Source and Copy. Sequence diagrams show each participant once, at the top.
- Android: the project menu (⋯) groups its sync settings under "Sync on all machines" as checkmarks instead of "On/Off (all machines)" labels. Choose ignored folders shows only while ignored folders sync, the Syncthing switch reads "Use Beep sync instead of Syncthing" (or back), and Remove project comes last after a line. All projects left the menu; Back goes there.

### Fixed

- Android: a connection to a machine that stops answering is dropped and dialed again after about 15 seconds instead of 30.
- Android: machines reconnect within seconds after the phone wakes instead of waiting out connections that died while it slept.
- Android: the project page's ⋯ menu no longer closes the moment it opens.
- Android: Projects no longer scrolls sideways when an open issue or pull request has many labels; the labels wrap under the title.
- Android: the project page has no blank band above its task field.
- Android: with more than 500 sessions in the lists, the app no longer asks every machine where each session works over and over.

### Added

- Android: pages slide in from the right, Back slides them in from the left, and tabs fade. Reduce motion turns it off.

### Performance

- Android: reads asked for once no longer stay live on the machines, and live views no one reads again are dropped after half a minute.
