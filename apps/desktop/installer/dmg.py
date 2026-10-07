# dmgbuild settings for the Beep installer. dmgbuild writes the window layout
# itself, so it needs no Finder or GUI session. Run through
# scripts/package-macos-dmg.sh, which passes app, background and icon.
import os.path

app = defines["app"]  # noqa: F821
background = defines["background"]  # noqa: F821
icon = defines["icon"]  # noqa: F821

format = "UDZO"
filesystem = "HFS+"
files = [app]
symlinks = {"Applications": "/Applications"}
badge_icon = None

window_rect = ((200, 120), (720, 508))
default_view = "icon-view"
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False

icon_size = 96
text_size = 13
arrange_by = None
label_pos = "bottom"
icon_locations = {
    os.path.basename(app): (200, 266),
    "Applications": (520, 266),
}
