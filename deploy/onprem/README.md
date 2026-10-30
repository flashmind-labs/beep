# Beep on your own machine

One Beep server for one person, with its Postgres and a node that lets the
agent run shell commands and coding agents in the `work` volume.

```sh
cp .env.example .env        # set POSTGRES_PASSWORD and BEEP_PROVIDER_KEY
docker compose up -d --build
docker compose exec -u beep server beep-server link /data/config.toml
```

The last command prints a QR code: scan it with Beep on your phone. The first
phone to connect owns the server (or only the key in `BEEP_OWNER`); other
phones and computers are refused until the owner links them under Settings,
Linked devices. The node links itself to the owner once the phone connects.

To use Claude Code or Codex on the node, sign in once:
`docker compose exec -it node claude` (or `codex`).

Logs: `docker compose logs -f server`. Data lives in the named volumes, so
`docker compose down` keeps it and `docker compose down -v` deletes it.
