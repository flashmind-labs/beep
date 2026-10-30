# Beep on your own machine

One Beep per Docker host: the server, its Postgres and a node that lets the
agent run shell commands and coding agents in the `work` volume.

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
cannot. `./setup.sh --admin` (or `BEEP_ADMIN=1`) writes `unlimited`.

To use Claude Code or Codex on the node, sign in once:
`docker compose exec -it -u beep node claude` (or `codex`).

Logs: `docker compose logs -f server`. Data lives in named volumes, so
`docker compose down` keeps it and `docker compose down -v` deletes it.
