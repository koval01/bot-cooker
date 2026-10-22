# Troubleshooting

Start with `bot-cooker-ctl status` and `bot-cooker-ctl logs`. The installer
itself diagnoses the common certificate problems below at the end of an
installation.

## The installer says "no answer yet"

- **DNS**: `getent hosts trap.example.com` on the server must print the
  server's public address. With a CDN, the record must be *DNS only*.
- **Firewall**: TCP 80 and 443 must be open in the provider's firewall or
  security group, not only on the host. Let's Encrypt validates over port 80
  or 443 from the internet.
- **Certificate**: `sudo docker compose -p bot-cooker logs caddy` shows the
  ACME exchange. Rate limits apply if you re-create the same hostname many
  times in a short period.

## `502 Bad Gateway` from Caddy

bot-cooker itself is not running. Check
`sudo docker compose -p bot-cooker logs bot-cooker`. A configuration error
names the field; fix it and re-run the installer.

## A port is already in use

Something else (often a web server installed with the image) listens on 80 or
443: `sudo ss -ltnp 'sport = :80'`. Stop it or use a dedicated server.

## Pages load very slowly

That is the tarpit. `robots.txt` and static files answer at once; most other
paths answer slowly on purpose. To test without delays, run locally with
`tarpit.initial_delay_ms: 0` and `tarpit.delay_ms: 0`.

## The persona changed after an upgrade

The key is the same but the release changed how personas are derived; the
changelog mentions it. See [Upgrades and keys](upgrades.md).

## Statistics show only one address

A proxy in front of bot-cooker does not pass the client address. See
[Behind an existing proxy](../deploy/behind-proxy.md).

## Still stuck

Open an issue with the *Bug report* template. Include the version and the
relevant log lines, but never your master key or admin token.
