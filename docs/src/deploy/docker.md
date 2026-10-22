# Docker Compose by hand

The installer writes a small Compose project; you can also run the files in
the repository directly.

## HTTPS with Caddy, built from source

`docker-compose.caddy.yml` builds the image from the checkout and puts Caddy
in front of it:

```bash
git clone https://github.com/koval01/bot-cooker && cd bot-cooker
DOMAIN=trap.example.com \
COOKER_MASTER_KEY="$(openssl rand -hex 32)" \
COOKER_ADMIN_TOKEN="$(openssl rand -hex 24)" \
docker compose -f docker-compose.caddy.yml up -d --build
```

Keep the key and token: put them in a `.env` file next to the compose file
(Compose reads it automatically) so that restarts keep the same persona.

The bundled `Caddyfile` turns off HTTP/3 and strips Caddy's `Via` header, so
responses carry the persona's own server headers. `flush_interval -1` makes
Caddy pass the slow trickle through as it is produced; do not remove it.

## Plain HTTP

`docker-compose.yml` runs only bot-cooker on port 8080, for local testing or
behind a proxy you already have. Change the port mapping to `"80:8080"` for
a public port-80 trap.

## The released image

```bash
docker run -d --name bot-cooker -p 80:8080 \
  -e COOKER_MASTER_KEY="$(openssl rand -hex 32)" \
  ghcr.io/koval01/bot-cooker:latest
```

Images are published for `linux/amd64` and `linux/arm64` on every release.
The container runs as an unprivileged user and reads
`/app/bot-cooker.yaml`; mount your own file there to change settings (see the
[configuration reference](../operate/configuration.md)).

## Open files

A tarpit holds many idle connections. The compose files raise the open-file
limit to 65535; keep that if you write your own. bot-cooker lowers its
connection cap to fit whatever limit it gets and logs when it does.
