# Behind an existing proxy

If TLS is already terminated somewhere else — a load balancer, an nginx you
run, a CDN — run bot-cooker in plain HTTP mode and point the proxy at it:

```bash
curl -fsSL https://raw.githubusercontent.com/koval01/bot-cooker/main/deploy/install.sh \
  | sudo bash -s -- --http-only
```

This publishes the trap on port 80 of the server. Requirements for the proxy:

- **No response buffering.** The slow trickle is the whole point; a proxy
  that buffers the response until it is complete defeats it and holds memory
  for each client. In nginx use `proxy_buffering off;`, in Caddy
  `flush_interval -1`.
- **Long timeouts.** Allow responses that last up to `tarpit.max_seconds`
  (20 minutes by default), e.g. `proxy_read_timeout 1300s;` in nginx.
- **Client address.** Pass the client address in `X-Forwarded-For` or
  `X-Real-IP` so that statistics show the scanners and not the proxy.
- **Its own headers.** A proxy that adds `Via`, `Server` or other headers of
  its own makes the persona less convincing; remove them where you can.

A CDN in front of the trap (for example a Cloudflare *proxied* record)
changes what scanners see — its headers, its TLS — and may cache or cut slow
responses. Prefer DNS-only records for trap hostnames.

## Next to a real site

If the proxy also serves a real site, send only junk paths to the trap, and
use the allowlist with the CIDR ranges of your own users if any of them can
reach it. See [Ethics and responsible use](../ethics.md).
