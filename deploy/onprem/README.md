# Beep on your own machine

One Beep per Docker host: the server, its Postgres and a node, the user's
environment. The node is Ubuntu: the agent runs code, shell commands and
coding agents there as `user`, with passwordless sudo and an open network,
and keeps the user's files in `/home/user` (the `node-home` volume). Files
and `pip install` packages in the home stay; `sudo apt-get install` lasts
until the container is recreated from a new image.

```sh
./setup.sh
```

It generates the secrets, writes `.env`, asks for the API keys it cannot
make (Enter skips), builds or pulls the images, starts everything and prints
a QR code. Scan it with Beep on your phone. The first phone to connect owns
the server (or only the key in `BEEP_OWNER`); other phones and computers are
refused until the owner links them under Settings, Linked devices. The node
links itself to the owner once the phone connects. Run it again any time: it
keeps what `.env` holds. `docker compose exec -u beep server beep-server
link /data/config.toml` shows a fresh code.

Unattended, every setting in `.env.example` can come from the environment,
and nothing is asked without a terminal or with `BEEP_SETUP_NONINTERACTIVE=1`:

```sh
BEEP_PROVIDER_KEY=sk-or-v1-... BEEP_OWNER=<phone endpoint id> ./setup.sh
```

The server and the node cap their outgoing traffic at `BEEP_EGRESS_LIMIT`
bytes per second (default `1M`, 1 MiB/s) and refuse to start when they
cannot. `./setup.sh --admin` (or `BEEP_ADMIN=1`) writes `unlimited`. Sudo in
the node cannot lift the cap; `docker exec` on the host can.
On a hosted Beep VM, the 1 MiB/s bandwidth cap is applied outside the VM.

To use Claude Code or Codex on the node, sign in once:
`docker compose exec -it -u user node claude` (or `codex`).

## The environment on another machine

The node connects over iroh, so the same image runs anywhere with Docker.
Get the server's id with `docker compose exec server cat /link/server.id`,
then on the other machine:

```sh
docker volume create beep-home
docker run --rm -it -e BEEP_EGRESS_LIMIT=unlimited -v beep-home:/home/user \
    beep-node:local --dir /home/user/.config/beep-node \
    link --server <server id> --name "laptop"
```

Paste the printed link in Beep on the phone (Settings, Linked devices, Type a
code instead), then start it:

```sh
docker run -d --name beep-env --restart unless-stopped \
    -e BEEP_EGRESS_LIMIT=unlimited -v beep-home:/home/user \
    beep-node:local --dir /home/user/.config/beep-node run
```

For a cap, drop `BEEP_EGRESS_LIMIT=unlimited` and add `--cap-add NET_ADMIN`.
Keep one environment per user: with two online, either may be used. Stop the
bundled one with `docker compose stop node`.

Logs: `docker compose logs -f server`. Data lives in named volumes, so
`docker compose down` keeps it and `docker compose down -v` deletes it.
