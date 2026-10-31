#!/bin/sh
# Starts the user's environment: caps outgoing traffic (see egress.sh), then
# runs beep-node as `user` in /home/user.
#
# NET_ADMIN is removed from the bounding set, so even root (through sudo)
# cannot change the cap from inside: the agent can install packages but not
# lift it. Anyone with `docker exec` on the host still can. Every other
# capability Docker grants stays available to sudo, which apt needs.
set -eu
beep-egress
if [ "$(id -u)" = 0 ]; then
    # A volume from before, or a new one, may be owned by root.
    chown user:user /home/user
    [ -d /home/user/work ] && chown user:user /home/user/work
    exec setpriv --reuid user --regid user --init-groups \
        --bounding-set=-net_admin \
        env HOME=/home/user USER=user LOGNAME=user beep-node "$@"
fi
exec beep-node "$@"
