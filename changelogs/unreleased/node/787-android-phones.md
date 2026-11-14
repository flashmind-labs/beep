### Added

- Node: an Android phone connected to the machine over adb, by USB or
  wireless debugging once debugging is allowed on the phone, is listed with
  the machine's devices. The Device tab and agents' `device` tool show its
  screen and tap, swipe, type and press keys on it. A phone cannot be booted
  or shut down from Beep.
- Node: the machine tells desktops when adb sees a phone or emulator connect
  or disconnect, so the device list updates without waiting for a refresh.
