# Configuration reference

Every setting has a built-in default, so bot-cooker runs without any
configuration file. The repository's
[`bot-cooker.yaml`](https://github.com/koval01/bot-cooker/blob/main/bot-cooker.yaml)
lists them all with comments. Unknown keys are rejected at startup, so a typo
fails loudly instead of being ignored.

```bash
bot-cooker --config bot-cooker.yaml --check   # validate and print the persona
```

## Environment variables

| Variable | Overrides | Notes |
|---|---|---|
| `COOKER_MASTER_KEY` | `deception.master_key` | the instance's identity; keep it secret and stable |
| `COOKER_ADMIN_TOKEN` | `stats.admin_token` | token for the stats endpoint |
| `COOKER_HTTP_BIND` | `http.bind` | e.g. `0.0.0.0:80` |
| `COOKER_TEMPLATES_DIR` | `deception.templates_dir` | e.g. a mounted content pack |
| `COOKER_MAX_CONCURRENT` | `limits.max_concurrent` | the installer sets it from the server's memory |

## Command line

| Flag | Meaning |
|---|---|
| `--config <file>` | configuration file (default `bot-cooker.yaml` if present) |
| `--check` | validate the configuration, print the persona and route count, exit |
| `--log-file <file>` | append logs to a file instead of standard error |
| `--service` | (Windows) run as a service; used by the service installer |
| `--version` | print the version |

## `http`

| Key | Default | Meaning |
|---|---|---|
| `enabled` | `true` | serve plain HTTP (the only listener) |
| `bind` | `0.0.0.0:8080` | address and port |

TLS is terminated by a reverse proxy in front of bot-cooker (Caddy in the
standard deployment).

## `tarpit`

| Key | Default | Meaning |
|---|---|---|
| `initial_delay_ms` | `350` | typical time until the headers: the application's response time |
| `initial_delay_jitter` | `0.5` | spread of that time |
| `chunk_bytes` | `48` | typical size of one piece of the body (pieces stay within 1–8×) |
| `delay_ms` | `800` | typical pause between pieces |
| `delay_jitter` | `0.4` | spread of the pauses |
| `abort_probability` | `0.15` | share of responses cut short |
| `max_bytes` | `4194304` | bytes sent to one client before giving up (4 MiB) |
| `max_seconds` | `1200` | hard cap per connection (20 min) |
| `max_idle_ms` | `6000` | longest silence on an open connection |

The trap answers like a real application: headers after a few hundred
milliseconds, then a body that trickles out unevenly — brisk stretches,
sluggish ones, stalls, and the occasional retransmission-like hiccup followed
by a burst. Keeping every silence under `max_idle_ms` matters: a client that
hears nothing for longer than its read timeout decides the server is dead and
leaves, while one that keeps receiving bytes keeps waiting. Each instance
draws its own distributions inside these bounds. Setting the delays to `0`
turns the tarpit off (useful for testing).

## `deception`

| Key | Default | Meaning |
|---|---|---|
| `master_key` | empty | **set a long random secret.** Empty means a new random key on every start, so the persona changes on each restart |
| `config_epoch_hours` | `168` | how often configuration-like content rotates (weekly) |
| `log_epoch_hours` | `8` | how often logs move forward |
| `epoch_jitter_hours` | `6` | per-instance fuzz on epoch boundaries |
| `persona_era_days` | `0` | `0`: the persona lives as long as the key; `N`: a new identity every N days |
| `templates_dir` | none | directory overlaid on the built-in templates and data, e.g. a [content pack](content-packs.md) |
| `host_os` | `auto` | the OS scanners see: `auto`, `unix` or `windows`; personas only use servers that run on it |

## `routes`

Leave it unset: the persona decides which paths exist. Setting it
**replaces** the persona's route table; the first matching pattern wins, `*`
matches any run of characters, and unmatched paths get the persona's 404.

```yaml
routes:
  - { pattern: "/.env",         profile: env }
  - { pattern: "/.git/*",       profile: git }
  - { pattern: "*.sql",         profile: sql_dump }
  - { pattern: "/my-panel/*",   profile: labyrinth }
```

Profiles: `env`, `git`, `sql_dump`, `log`, `login` (alias `wp_login`),
`php_info`, `server_status`, `joomla`, `drupal`, `json`, `sitemap`, `rss`,
`secrets`, `labyrinth`, `not_found`, `forbidden`, `blank`, `asset`. Content
still follows the persona: `env` renders a `.env`, `wp-config.php` or
`database.yml` depending on the path and the stack.

## `allowlist`

| Key | Default | Meaning |
|---|---|---|
| `cidrs` | `[]` | client networks that get a fast plain 404 instead of the trap |
| `user_agents` | `[]` | User-Agent substrings to spare (spoofable; avoid) |

Empty on purpose: on a dedicated trap hostname nothing legitimate arrives,
and sparing clients by User-Agent would let any scanner walk out by sending
`User-Agent: Googlebot`. If you run the trap next to a real site, allowlist
your users by CIDR.

## `limits`

| Key | Default | Meaning |
|---|---|---|
| `max_concurrent` | `8000` | connections held at most (~30 KiB of RAM and one open file each; 8000 ≈ 250 MiB) |

Behind the Caddy sidecar, each held connection also costs Caddy about
250 KiB, so the installer sets the cap from the server's memory (about 300
KiB per connection over HTTPS, 40 KiB over plain HTTP, 350 MiB left for the
system): roughly 2000 on a 1 GiB server.

Past the cap, new requests get fast answers instead of another held slot. At
startup the cap is lowered to fit the open-files limit (`ulimit -n`,
`LimitNOFILE`), which bot-cooker raises as far as the hard limit allows.

## `stats`

| Key | Default | Meaning |
|---|---|---|
| `report_interval_secs` | `60` | how often a summary line is logged (`0` = never) |
| `admin_path` | `/_cooker/stats` | path of the token-protected JSON endpoint |
| `admin_token` | empty | token; empty means a random one, printed at startup |
| `max_tracked_ips` | `50000` | size cap of the per-IP table |

See [Monitoring and stats](monitoring.md).
