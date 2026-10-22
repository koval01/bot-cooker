# Rolling out a trap

A checklist for putting a new build or a new instance in production. Every
step runs against hosts you own.

## 1. Before deploying

- CI on the commit is green: lint, tests on Linux and Windows, the Windows
  service job, the Docker job.
- Each instance has its own long random `master_key` (the installers and
  `openssl rand -hex 32` give one). Never reuse a key across hosts: the key
  is the persona.
- `bot-cooker --check` on the target config prints the persona you expect
  (stack, front server, route count).
- `host_os` matches the OS whose TCP stack scanners will see (`auto` unless
  connections terminate on another OS than the one bot-cooker runs on).
- Upgrading from an older build changes every key's content once (new rand
  sampling, new version data). Prefer a fresh key and hostname over
  upgrading an instance scanners already know.

## 2. One staging instance

Deploy one instance on a dedicated trap hostname and let it run for a few
days before touching the others.

- Service identification looks like the persona's server:
  `nmap -sV -Pn -p <port> <host>` names the server and version the persona
  claims, not an unknown service.
- Content checks see the persona's stack:
  `nuclei -u http(s)://<host> -tags tech -silent` reports the same stack,
  server and versions as `--check`.
- Protocol edge cases come from the persona's server: `tools/capture/grab.py
  <host> <port> <outdir>` saves the raw answers to a fixed probe set; read
  the 400/404/HTTP-1.0 answers and compare them with a real server of the
  same kind if you have one.
- The process stays healthy under load: run
  `cargo run --release --example load -- --addr <host>:<port> --conns 4000 --secs 120`
  from another box you own and watch memory (about 30 KiB per held
  connection) and CPU. The stats endpoint shows `active` near the number of
  held connections.

## 3. Watch it

- `GET <admin_path>` with `X-Admin-Token` returns the counters; the log gets
  one `report:` line per `stats.report_interval_secs`.
- Memory stays bounded by `limits.max_concurrent` (8000 by default, lowered
  at startup to fit the open-files limit; the log says so when it happens).
- The process restarts cleanly: `docker stop`, `systemctl stop` or
  `Stop-Service` ends it within a few seconds.

## 4. Roll out

- One host at a time, each with its own key and hostname.
- Keep the previous build's image or binary until the new one has run for a
  day.

## Known gaps

- With the standard Caddy sidecar, the HTTP/1.1 response headers that reach
  clients are reordered and re-cased by Caddy (Go's `net/http` sorts them and
  writes `Etag`, `X-Xss-Protection`), which undoes the front server's own
  header style. Caddy also answers port 80 itself (`308 Permanent Redirect`
  with `Server: Caddy`), answers malformed requests with Go's own `400`, and
  its TLS handshake has Go's fingerprint. Behind it, HTTP/1.1 clients also
  see the body's trickle as many small chunks, because Caddy writes each
  piece as it arrives (direct clients get buffer-sized chunks). Terminating
  TLS in bot-cooker itself is planned to remove these.
- The site front (the home page and content pages) is drawn by bot-cooker's
  own themes, not by captured stock output of the CMS or server. A careful
  comparison with a real install of the same software can tell them apart.
- `cooker-pack` has only been run with `--dry-run`, never against the API.
